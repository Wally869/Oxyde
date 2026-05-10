use crate::abi::{self, FunctionAbi};
use crate::bytecode::BytecodeBuilder;
use crate::layout::{StorageFieldKind, StorageLayout};
use crate::opcodes::Opcode;
use oxyde_parser::typing::{Arg, Mutability};
use std::collections::HashMap;
use syn::{Expr, ImplItem, ImplItemFn, ItemImpl, ItemStruct, Stmt};

pub struct CompiledContract {
    pub name: String,
    pub runtime_bytecode: Vec<u8>,
    pub creation_bytecode: Vec<u8>,
    pub functions: Vec<FunctionAbi>,
    pub constructor: Option<FunctionAbi>,
    pub layout: StorageLayout,
}

/// ABI info for an external contract, used to resolve cross-contract calls.
#[derive(Debug)]
pub struct ExternalContractAbi {
    pub functions: Vec<FunctionAbi>,
}

impl ExternalContractAbi {
    pub fn find_function(&self, name: &str) -> Option<&FunctionAbi> {
        self.functions.iter().find(|f| f.name == name)
    }
}

type ExternalAbis = HashMap<String, ExternalContractAbi>;

pub fn compile(
    contract_struct: &ItemStruct,
    contract_impl: &ItemImpl,
) -> Result<CompiledContract, String> {
    compile_with_externals(contract_struct, contract_impl, &HashMap::new())
}

pub fn compile_with_externals(
    contract_struct: &ItemStruct,
    contract_impl: &ItemImpl,
    external_abis: &ExternalAbis,
) -> Result<CompiledContract, String> {
    let name = contract_struct.ident.to_string();
    let layout = StorageLayout::from_struct(contract_struct)?;

    // Collect public methods from impl
    let methods: Vec<&ImplItemFn> = contract_impl
        .items
        .iter()
        .filter_map(|item| {
            if let ImplItem::Fn(method) = item {
                if matches!(method.vis, syn::Visibility::Public(_)) {
                    return Some(method);
                }
            }
            None
        })
        .collect();

    let constructor_method = methods.iter().find(|m| m.sig.ident == "constructor").copied();
    let regular_methods: Vec<&ImplItemFn> = methods
        .iter()
        .filter(|m| m.sig.ident != "constructor")
        .copied()
        .collect();

    // Build FunctionAbi for each regular method
    let mut function_abis: Vec<FunctionAbi> = Vec::new();
    for method in &regular_methods {
        let method_args = extract_typed_args(method);

        let output_type = match &method.sig.output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, ty) => Some(abi::syn_type_to_string(ty)),
        };

        let mutability = infer_mutability(method);

        let fabi = FunctionAbi::new(
            &method.sig.ident.to_string(),
            &method_args,
            output_type.as_deref(),
            mutability,
        )?;
        function_abis.push(fabi);
    }

    // Build constructor ABI if present
    let constructor_abi = if let Some(ctor) = constructor_method {
        let ctor_args = extract_typed_args(ctor);
        Some(FunctionAbi::new(
            "constructor",
            &ctor_args,
            None,
            Mutability::Mutable,
        )?)
    } else {
        None
    };

    // --- Compile runtime bytecode ---
    let mut runtime = BytecodeBuilder::new();

    // Selector dispatch preamble: load 4-byte selector from calldata
    // PUSH1 0x00, CALLDATALOAD, PUSH1 0xe0, SHR
    runtime.emit_push_u8(0x00);
    runtime.emit(Opcode::CallDataLoad);
    runtime.emit_push_u8(0xe0);
    runtime.emit(Opcode::LogicalRightShift);

    // Create labels and emit dispatch table
    let mut func_labels = Vec::new();
    for fabi in &function_abis {
        let label = runtime.create_label();
        runtime.emit(Opcode::Dup(1));
        runtime.emit_push_u32(u32::from_be_bytes(fabi.selector));
        runtime.emit(Opcode::Eq);
        runtime.emit_jumpi(label);
        func_labels.push(label);
    }

    // Fallback: revert
    runtime.emit_push_u8(0x00);
    runtime.emit_push_u8(0x00);
    runtime.emit(Opcode::Revert);

    // Compile each function body
    for (i, method) in regular_methods.iter().enumerate() {
        runtime.place_label(func_labels[i]);
        runtime.emit(Opcode::Pop); // remove the selector copy
        compile_function(&mut runtime, method, &layout, external_abis)?;
    }

    let runtime_bytecode = runtime.finalize();

    // --- Compile creation bytecode ---
    let creation_bytecode = if let Some(ctor) = constructor_method {
        let ctor_args = extract_typed_args(ctor);
        compile_creation_code(&runtime_bytecode, &ctor_args, &layout)?
    } else {
        BytecodeBuilder::emit_creation_code(&runtime_bytecode)
    };

    Ok(CompiledContract {
        name,
        runtime_bytecode,
        creation_bytecode,
        functions: function_abis,
        constructor: constructor_abi,
        layout,
    })
}

// ---- Helper: extract non-receiver args from a method ----

pub fn extract_typed_args(method: &ImplItemFn) -> Vec<Arg> {
    method
        .sig
        .inputs
        .iter()
        .filter_map(|input| {
            if let syn::FnArg::Typed(pat_type) = input {
                let name = if let syn::Pat::Ident(ident) = &*pat_type.pat {
                    Some(ident.ident.clone())
                } else {
                    None
                };
                Some(Arg {
                    name,
                    r#type: (*pat_type.ty).clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

// ---- Helper: infer mutability from receiver ----

pub fn infer_mutability(method: &ImplItemFn) -> Mutability {
    for input in &method.sig.inputs {
        if let syn::FnArg::Receiver(r) = input {
            return if r.mutability.is_some() {
                Mutability::Mutable
            } else {
                Mutability::View
            };
        }
    }
    Mutability::Pure
}

// ---- Compile a single function body into the builder ----

fn compile_function(
    b: &mut BytecodeBuilder,
    method: &ImplItemFn,
    layout: &StorageLayout,
    externals: &ExternalAbis,
) -> Result<(), String> {
    let args = extract_typed_args(method);
    let stmts = &method.block.stmts;
    let has_return = !matches!(method.sig.output, syn::ReturnType::Default);

    let mut locals: Vec<String> = Vec::new();

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;
        match stmt {
            Stmt::Local(local) => {
                // let x = expr;
                let name = if let syn::Pat::Ident(pat_ident) = &local.pat {
                    pat_ident.ident.to_string()
                } else {
                    return Err("unsupported let pattern (only simple ident supported)".to_string());
                };

                if let Some(init) = &local.init {
                    compile_expr(b, &init.expr, layout, &args, &mut locals, externals)?;
                } else {
                    return Err("let binding without initializer not supported".to_string());
                }

                // Value is now on the stack; track the local name
                locals.push(name);
            }
            Stmt::Expr(expr, semi) => {
                compile_expr(b, expr, layout, &args, &mut locals, externals)?;

                if is_last && semi.is_none() && has_return {
                    // Last expression without semicolon is the return value.
                    // Clean up locals before returning (they are below the return value).
                    // Stack: [local0, local1, ..., localN, retval]
                    // We need to remove the locals. Use SWAP+POP for each.
                    for _ in &locals {
                        // retval is on top; swap it down past one local, pop that local
                        b.emit(Opcode::Swap(1));
                        b.emit(Opcode::Pop);
                    }
                    // Store at memory[0] and return 32 bytes.
                    b.emit_push_u8(0x00);
                    b.emit(Opcode::MStore);
                    b.emit_push_u8(0x20);
                    b.emit_push_u8(0x00);
                    b.emit(Opcode::Return);
                } else if semi.is_some() {
                    // Expression statement with semicolon: result (if any) is discarded
                    // For void-returning expressions (set, set_value, if-revert), nothing to do.
                    // For expressions that do push a value, we'd need to pop.
                    // The current void expressions (SStore) don't push, so nothing needed here.
                }
                // Expression statements that produce a value (e.g. a set() call that
                // leaves nothing on the stack) need no extra handling for the MVP.
            }
            Stmt::Macro(stmt_mac) => {
                compile_stmt_macro(b, &stmt_mac.mac)?;
            }
            _ => return Err("unsupported statement type".to_string()),
        }
    }

    if !has_return {
        // Pop all locals before stopping
        for _ in &locals {
            b.emit(Opcode::Pop);
        }
        b.emit(Opcode::Stop);
    }

    Ok(())
}

// ---- Recursive expression compiler ----

fn compile_expr(
    b: &mut BytecodeBuilder,
    expr: &Expr,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    externals: &ExternalAbis,
) -> Result<(), String> {
    match expr {
        Expr::MethodCall(mc) => {
            if let Some(field_name) = get_self_field_name(&mc.receiver) {
                let field = layout
                    .get_field(&field_name)
                    .ok_or_else(|| format!("unknown field: {field_name}"))?;
                match (field.kind.clone(), mc.method.to_string().as_str()) {
                    (StorageFieldKind::Item, "get_value") => {
                        b.emit_push_usize(field.slot as usize);
                        b.emit(Opcode::SLoad);
                    }
                    (StorageFieldKind::Item, "set") => {
                        let val_expr = mc
                            .args
                            .first()
                            .ok_or_else(|| "set() requires one argument".to_string())?;
                        compile_expr(b, val_expr, layout, args, locals, externals)?;
                        b.emit_push_usize(field.slot as usize);
                        b.emit(Opcode::SStore);
                    }
                    (StorageFieldKind::Map, "get") => {
                        // self.field.get(&key) -> keccak256(key ++ slot), SLOAD
                        let key_expr = mc
                            .args
                            .first()
                            .ok_or_else(|| "get() requires one argument".to_string())?;
                        let slot = field.slot as usize;
                        emit_keccak_map_slot(b, layout, args, locals, key_expr, slot, externals)?;
                        b.emit(Opcode::SLoad);
                    }
                    (StorageFieldKind::Map, "set_value") => {
                        // self.field.set_value(&key, val)
                        // Stack goal: [val, slot_hash], then SSTORE
                        let key_expr = mc
                            .args
                            .first()
                            .ok_or_else(|| "set_value() requires two arguments".to_string())?;
                        let val_expr = mc
                            .args
                            .get(1)
                            .ok_or_else(|| "set_value() requires two arguments".to_string())?;
                        let slot = field.slot as usize;

                        // Compile value first
                        compile_expr(b, val_expr, layout, args, locals, externals)?;
                        // Now compute the keccak slot hash on top of val
                        // Temporarily account for val on stack: we push key, mstore, push slot, mstore, sha3
                        // The val sits below; key computation is self-contained (no net stack change
                        // except producing the slot hash on top).
                        emit_keccak_map_slot_with_offset(b, layout, args, locals, key_expr, slot, 1, externals)?;
                        // Stack: [val, slot_hash]
                        b.emit(Opcode::SStore);
                    }
                    (StorageFieldKind::DoubleMap, "get") => {
                        // self.field.get(&k1, &k2)
                        // Slot = keccak256(k2 ++ keccak256(k1 ++ base_slot))
                        if mc.args.len() < 2 {
                            return Err("get() on DoubleMap requires two arguments".to_string());
                        }
                        let k1_expr = &mc.args[0];
                        let k2_expr = &mc.args[1];
                        let base_slot = field.slot as usize;

                        // Step 1: keccak256(k1 ++ base_slot)
                        // compile k1 — no extra stack items at this point
                        compile_expr(b, k1_expr, layout, args, locals, externals)?;
                        // mem[0..32] = k1
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::MStore);
                        // mem[32..64] = base_slot
                        b.emit_push_usize(base_slot);
                        b.emit_push_u8(0x20);
                        b.emit(Opcode::MStore);
                        // intermediate_hash = keccak256(mem[0..64])
                        b.emit_push_u8(0x40);
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::Sha3); // stack: [intermediate_hash]

                        // Step 2: keccak256(k2 ++ intermediate_hash)
                        // mem[32..64] = intermediate_hash (pops it from stack)
                        b.emit_push_u8(0x20);
                        b.emit(Opcode::MStore); // stack: []
                        // compile k2 — no extra stack items
                        compile_expr(b, k2_expr, layout, args, locals, externals)?;
                        // mem[0..32] = k2
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::MStore);
                        // final_hash = keccak256(mem[0..64])
                        b.emit_push_u8(0x40);
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::Sha3); // stack: [final_hash]

                        b.emit(Opcode::SLoad);
                    }
                    (StorageFieldKind::DoubleMap, "set_value") => {
                        // self.field.set_value(&k1, &k2, val)
                        if mc.args.len() < 3 {
                            return Err("set_value() on DoubleMap requires three arguments".to_string());
                        }
                        let k1_expr = &mc.args[0];
                        let k2_expr = &mc.args[1];
                        let val_expr = &mc.args[2];
                        let base_slot = field.slot as usize;

                        // compile val first — it sits at bottom during slot computation
                        compile_expr(b, val_expr, layout, args, locals, externals)?;
                        // stack: [val]

                        // Step 1: keccak256(k1 ++ base_slot)
                        // 1 extra item (val) on stack — use dummy so DUP depths for locals are correct
                        emit_compile_expr_with_offset(b, k1_expr, layout, args, locals, 1, externals)?;
                        // stack: [val, k1]
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::MStore); // stack: [val]
                        b.emit_push_usize(base_slot);
                        b.emit_push_u8(0x20);
                        b.emit(Opcode::MStore); // stack: [val]
                        b.emit_push_u8(0x40);
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::Sha3); // stack: [val, intermediate_hash]

                        // mem[32..64] = intermediate_hash
                        b.emit_push_u8(0x20);
                        b.emit(Opcode::MStore); // stack: [val]

                        // compile k2 — 1 extra item (val) on stack
                        emit_compile_expr_with_offset(b, k2_expr, layout, args, locals, 1, externals)?;
                        // stack: [val, k2]
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::MStore); // stack: [val]
                        b.emit_push_u8(0x40);
                        b.emit_push_u8(0x00);
                        b.emit(Opcode::Sha3); // stack: [val, final_hash]

                        b.emit(Opcode::SStore);
                    }
                    (_, other) => return Err(format!("unsupported method: {other}")),
                }
            } else {
                return Err("unsupported method call receiver".to_string());
            }
        }

        Expr::Binary(bin) => {
            compile_expr(b, &bin.left, layout, args, locals, externals)?;
            compile_expr(b, &bin.right, layout, args, locals, externals)?;
            match bin.op {
                syn::BinOp::Add(_) => b.emit(Opcode::Add),
                syn::BinOp::Sub(_) => {
                    // EVM SUB: TOS - next. We compiled left first so TOS=right.
                    // SWAP so TOS=left, then SUB gives left - right.
                    b.emit(Opcode::Swap(1));
                    b.emit(Opcode::Sub);
                }
                syn::BinOp::Mul(_) => b.emit(Opcode::Mul),
                syn::BinOp::Div(_) => {
                    // EVM DIV: TOS / next. SWAP so TOS=left (dividend).
                    b.emit(Opcode::Swap(1));
                    b.emit(Opcode::Div);
                }
                // Comparisons: compiled left first, right second.
                // After SWAP1: TOS=left, next=right.
                // LT: TOS < next = left < right ✓
                // GT: TOS > next = left > right ✓
                syn::BinOp::Lt(_) => {
                    b.emit(Opcode::Swap(1));
                    b.emit(Opcode::LessThan);
                }
                syn::BinOp::Gt(_) => {
                    b.emit(Opcode::Swap(1));
                    b.emit(Opcode::GreaterThan);
                }
                syn::BinOp::Le(_) => {
                    // left <= right  =  !(left > right)
                    b.emit(Opcode::Swap(1));
                    b.emit(Opcode::GreaterThan);
                    b.emit(Opcode::IsZero);
                }
                syn::BinOp::Ge(_) => {
                    // left >= right  =  !(left < right)
                    b.emit(Opcode::Swap(1));
                    b.emit(Opcode::LessThan);
                    b.emit(Opcode::IsZero);
                }
                syn::BinOp::Eq(_) => {
                    b.emit(Opcode::Eq);
                }
                syn::BinOp::Ne(_) => {
                    b.emit(Opcode::Eq);
                    b.emit(Opcode::IsZero);
                }
                _ => return Err("unsupported binary operator".to_string()),
            }
        }

        Expr::Unary(unary) => {
            match unary.op {
                syn::UnOp::Not(_) => {
                    compile_expr(b, &unary.expr, layout, args, locals, externals)?;
                    b.emit(Opcode::IsZero);
                }
                _ => return Err("unsupported unary operator".to_string()),
            }
        }

        Expr::Call(call) => {
            if is_u256_from(call) {
                if let Some(inner) = call.args.first() {
                    compile_expr(b, inner, layout, args, locals, externals)?;
                } else {
                    return Err("U256::from() requires one argument".to_string());
                }
            } else if is_msg_sender(call) {
                b.emit(Opcode::Caller);
            } else if is_self_address(call) {
                b.emit(Opcode::Address);
            } else if is_msg_value(call) {
                b.emit(Opcode::CallValue);
            } else if let Some((contract_name, method_name)) = is_typed_external_call(call) {
                if let Some(contract_abi) = externals.get(&contract_name) {
                    let func = contract_abi
                        .find_function(&method_name)
                        .ok_or_else(|| format!("{contract_name} has no method '{method_name}'"))?;
                    let method_arg_count = call.args.len() - 1; // minus the target
                    if method_arg_count != func.inputs.len() {
                        return Err(format!(
                            "{contract_name}::{method_name}() expects {} args + target, got {} total",
                            func.inputs.len(),
                            call.args.len()
                        ));
                    }
                    compile_typed_external_call(b, call, func, layout, args, locals, externals)?;
                } else {
                    return Err(format!("unknown contract type: {contract_name}"));
                }
            } else if let Some((method_name, arg_count)) = is_external_call(call) {
                compile_external_call(b, call, &method_name, arg_count, layout, args, locals, externals)?;
            } else if let Some(n) = is_emit_log_raw(call) {
                compile_emit_log(b, call, n, layout, args, locals, externals)?;
            } else {
                return Err("unsupported function call".to_string());
            }
        }

        Expr::Path(path) => {
            let name = path
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();

            // Check locals first (most recent binding is at the top of the stack)
            if let Some(local_idx) = locals.iter().rposition(|l| l == &name) {
                // Depth from TOS: locals.len() - 1 - local_idx positions below TOS.
                // DUP(1) copies TOS, DUP(2) copies item one below TOS, etc.
                let depth = locals.len() - local_idx;
                if depth > 16 {
                    return Err(format!("local '{name}' is too deep on the stack (depth {depth})"));
                }
                b.emit(Opcode::Dup(depth as u8));
            } else if let Some(idx) = args
                .iter()
                .position(|a| a.name.as_ref().map(|n| n.to_string()) == Some(name.clone()))
            {
                b.emit_push_usize(4 + idx * 32);
                b.emit(Opcode::CallDataLoad);
            } else {
                return Err(format!("unknown variable: {name}"));
            }
        }

        Expr::Lit(lit) => {
            if let syn::Lit::Int(int_lit) = &lit.lit {
                let val: u64 = int_lit.base10_parse().map_err(|e: syn::Error| e.to_string())?;
                b.emit_push_usize(val as usize);
            } else {
                return Err("unsupported literal type".to_string());
            }
        }

        Expr::Paren(paren) => {
            compile_expr(b, &paren.expr, layout, args, locals, externals)?;
        }

        Expr::Reference(ref_expr) => {
            // &expr → just compile the inner expression
            compile_expr(b, &ref_expr.expr, layout, args, locals, externals)?;
        }

        Expr::If(if_expr) => {
            // if cond { body } [else { else_body }]
            compile_expr(b, &if_expr.cond, layout, args, locals, externals)?;
            b.emit(Opcode::IsZero); // ISZERO: skip body if cond is false
            let skip_label = b.create_label();
            b.emit_jumpi(skip_label);

            // Compile the then-block
            compile_block_stmts(b, &if_expr.then_branch.stmts, layout, args, locals, externals)?;

            if let Some((_else_tok, else_expr)) = &if_expr.else_branch {
                let end_label = b.create_label();
                b.emit_jump(end_label);
                b.place_label(skip_label);
                compile_expr(b, else_expr, layout, args, locals, externals)?;
                b.place_label(end_label);
            } else {
                b.place_label(skip_label);
            }
        }

        Expr::Block(block_expr) => {
            compile_block_stmts(b, &block_expr.block.stmts, layout, args, locals, externals)?;
        }

        Expr::Macro(mac) => {
            // panic!(...) or revert!(...) → PUSH0 PUSH0 REVERT
            let mac_name = mac.mac.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
            if mac_name == "panic" || mac_name == "revert" {
                b.emit(Opcode::Push0);
                b.emit(Opcode::Push0);
                b.emit(Opcode::Revert);
            } else {
                return Err(format!("unsupported macro: {mac_name}!()"));
            }
        }

        _ => return Err("unsupported expression kind".to_string()),
    }
    Ok(())
}

// ---- Compile the statements of a block (for if-bodies, block expressions) ----

fn compile_block_stmts(
    b: &mut BytecodeBuilder,
    stmts: &[Stmt],
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    externals: &ExternalAbis,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(expr, _semi) => {
                compile_expr(b, expr, layout, args, locals, externals)?;
            }
            Stmt::Local(local) => {
                let name = if let syn::Pat::Ident(pat_ident) = &local.pat {
                    pat_ident.ident.to_string()
                } else {
                    return Err("unsupported let pattern in block".to_string());
                };
                if let Some(init) = &local.init {
                    compile_expr(b, &init.expr, layout, args, locals, externals)?;
                } else {
                    return Err("let binding without initializer not supported".to_string());
                }
                locals.push(name);
            }
            Stmt::Macro(stmt_mac) => {
                compile_stmt_macro(b, &stmt_mac.mac)?;
            }
            _ => return Err("unsupported statement type in block".to_string()),
        }
    }
    Ok(())
}

fn compile_stmt_macro(b: &mut BytecodeBuilder, mac: &syn::Macro) -> Result<(), String> {
    let mac_name = mac.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
    if mac_name == "panic" || mac_name == "revert" {
        b.emit(Opcode::Push0);
        b.emit(Opcode::Push0);
        b.emit(Opcode::Revert);
        Ok(())
    } else {
        Err(format!("unsupported macro: {mac_name}!()"))
    }
}

// ---- Expression compile with stack offset ----

/// Compile an expression while accounting for `extra_stack` items already on the stack
/// above locals (needed for DUP depth adjustments).
fn emit_compile_expr_with_offset(
    b: &mut BytecodeBuilder,
    expr: &Expr,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    extra_stack: usize,
    externals: &ExternalAbis,
) -> Result<(), String> {
    for _ in 0..extra_stack {
        locals.push(String::from("\x00dummy"));
    }
    compile_expr(b, expr, layout, args, locals, externals)?;
    for _ in 0..extra_stack {
        locals.pop();
    }
    Ok(())
}

// ---- Keccak slot helpers ----

/// Emit code to compute keccak256(key ++ base_slot) following Solidity mapping layout.
/// The result (slot hash) is left on the stack.
/// `extra_stack` accounts for values already on the stack above locals (e.g. the value
/// for set_value). We use scratch memory at offsets that don't conflict with our memory layout.
/// We use memory[0x00..0x20] for key and memory[0x20..0x40] for the slot.
fn emit_keccak_map_slot(
    b: &mut BytecodeBuilder,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    key_expr: &Expr,
    base_slot: usize,
    externals: &ExternalAbis,
) -> Result<(), String> {
    emit_keccak_map_slot_with_offset(b, layout, args, locals, key_expr, base_slot, 0, externals)
}

/// Same as `emit_keccak_map_slot` but with `extra_stack` items already on the stack
/// above locals (needed for DUP depth adjustments when computing key from locals).
fn emit_keccak_map_slot_with_offset(
    b: &mut BytecodeBuilder,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    key_expr: &Expr,
    base_slot: usize,
    extra_stack: usize,
    externals: &ExternalAbis,
) -> Result<(), String> {
    // We need to temporarily inflate `locals` to account for extra_stack items
    // so DUP depth calculation for any locals referenced in key_expr is correct.
    // We do this by pushing dummy entries, compiling, then removing them.
    let dummy_count = extra_stack;
    for _ in 0..dummy_count {
        locals.push(String::from("\x00dummy"));
    }

    compile_expr(b, key_expr, layout, args, locals, externals)?;

    // Remove dummy entries
    for _ in 0..dummy_count {
        locals.pop();
    }

    // Stack: [..., key]
    // memory[0x00..0x20] = key (MSTORE stores 32-byte big-endian, left-padded)
    b.emit_push_u8(0x00);
    b.emit(Opcode::MStore); // mem[0..32] = key; stack: [...]

    // memory[0x20..0x40] = base_slot
    b.emit_push_usize(base_slot);
    b.emit_push_u8(0x20);
    b.emit(Opcode::MStore); // mem[32..64] = base_slot; stack: [...]

    // SHA3(offset=0, length=64) → keccak256(key ++ base_slot)
    b.emit_push_u8(0x40); // length = 64
    b.emit_push_u8(0x00); // offset = 0
    b.emit(Opcode::Sha3); // stack: [..., slot_hash]

    Ok(())
}

// ---- Helpers for expression pattern recognition ----

/// If expr is `self.<field>`, returns the field name.
fn get_self_field_name(expr: &Expr) -> Option<String> {
    if let Expr::Field(field_expr) = expr {
        if let Expr::Path(path) = &*field_expr.base {
            if path.path.is_ident("self") {
                if let syn::Member::Named(ident) = &field_expr.member {
                    return Some(ident.to_string());
                }
            }
        }
    }
    None
}

/// Returns true if the call is `U256::from(...)`.
fn is_u256_from(call: &syn::ExprCall) -> bool {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 2 {
            return segs[0].ident == "U256" && segs[1].ident == "from";
        }
    }
    false
}

/// Returns true if the call is `msg_sender()`.
fn is_msg_sender(call: &syn::ExprCall) -> bool {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 1 {
            return segs[0].ident == "msg_sender";
        }
    }
    false
}

/// Returns true if the call is `self_address()`.
fn is_self_address(call: &syn::ExprCall) -> bool {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 1 {
            return segs[0].ident == "self_address";
        }
    }
    false
}

/// Returns true if the call is `msg_value()`.
fn is_msg_value(call: &syn::ExprCall) -> bool {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 1 {
            return segs[0].ident == "msg_value";
        }
    }
    false
}

/// Returns `Some((contract_name, method_name))` if the call looks like `ContractName::method(...)`
/// and is not a known intrinsic type.
fn is_typed_external_call(call: &syn::ExprCall) -> Option<(String, String)> {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 2 {
            let type_name = segs[0].ident.to_string();
            let method_name = segs[1].ident.to_string();
            // Don't match known intrinsics
            if type_name != "U256" && type_name != "ExternalCall" {
                return Some((type_name, method_name));
            }
        }
    }
    None
}

/// Returns `Some((method_name, arg_count))` if the call is `ExternalCall::call_N` or
/// `ExternalCall::call_void_N` where N is 1..=3.
fn is_external_call(call: &syn::ExprCall) -> Option<(String, usize)> {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 2 && segs[0].ident == "ExternalCall" {
            let method = segs[1].ident.to_string();
            if let Some(n) = method.strip_prefix("call_void_") {
                if let Ok(count) = n.parse::<usize>() {
                    return Some((method, count));
                }
            } else if let Some(n) = method.strip_prefix("call_") {
                if let Ok(count) = n.parse::<usize>() {
                    return Some((method, count));
                }
            }
        }
    }
    None
}

/// Returns `Some(n)` if the call is `emit_logN_raw` where N is 1, 2, or 3.
fn is_emit_log_raw(call: &syn::ExprCall) -> Option<u8> {
    if let Expr::Path(path) = &*call.func {
        let segs: Vec<_> = path.path.segments.iter().collect();
        if segs.len() == 1 {
            let name = segs[0].ident.to_string();
            if let Some(rest) = name.strip_prefix("emit_log") {
                if let Some(n_str) = rest.strip_suffix("_raw") {
                    if let Ok(n) = n_str.parse::<u8>() {
                        if (1..=3).contains(&n) {
                            return Some(n);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Compile an `emit_logN_raw(topic0, ..., topicN-1, data)` call into EVM LOGN opcode sequence.
///
/// Memory layout: data value is stored at mem[0x00..0x20] (scratch space).
/// Topics are pushed in reverse order (last topic first) to satisfy EVM stack ordering.
fn compile_emit_log(
    b: &mut BytecodeBuilder,
    call: &syn::ExprCall,
    num_topics: u8,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    externals: &ExternalAbis,
) -> Result<(), String> {
    let expected_args = num_topics as usize + 1;
    if call.args.len() != expected_args {
        return Err(format!(
            "emit_log{num_topics}_raw expects {expected_args} arguments, got {}",
            call.args.len()
        ));
    }

    // Last arg is the non-indexed data value; store it in mem[0x00..0x20]
    let data_expr = &call.args[call.args.len() - 1];
    compile_expr(b, data_expr, layout, args, locals, externals)?;
    b.emit_push_u8(0x00);
    b.emit(Opcode::MStore);

    // Push topics in reverse order so that after LOG pops (offset, size, topic0, topic1, ...)
    // with offset at TOS, we need: TOS=offset, next=size, then topics in ascending order.
    // We push: topicN-1, ..., topic1, topic0, size=0x20, offset=0x00
    for i in (0..num_topics as usize).rev() {
        compile_expr(b, &call.args[i], layout, args, locals, externals)?;
    }

    b.emit_push_u8(0x20); // size = 32 bytes
    b.emit_push_u8(0x00); // offset = 0

    match num_topics {
        1 => b.emit(Opcode::Log1),
        2 => b.emit(Opcode::Log2),
        3 => b.emit(Opcode::Log3),
        _ => return Err(format!("unsupported log topic count: {num_topics}")),
    }

    Ok(())
}

/// Compile an `ExternalCall::call_N` or `ExternalCall::call_void_N` into EVM CALL sequence.
///
/// Call args: `[target: Address, selector: u32, arg0: U256, arg1: U256, ...]`
/// Memory layout used:
///   mem[0x80..0xA0]           = selector << 224 (4 bytes left-aligned in 32-byte word)
///   mem[0x84 + i*32 .. +32]  = argI             (MSTORE at 0x84 + i*32)
/// Return data (if any) is placed at mem[0x00..0x20] and loaded onto the stack.
fn compile_external_call(
    b: &mut BytecodeBuilder,
    call: &syn::ExprCall,
    method_name: &str,
    arg_count: usize,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    externals: &ExternalAbis,
) -> Result<(), String> {
    let is_void = method_name.starts_with("call_void_");
    let has_return = !is_void;
    let expected_args = 2 + arg_count;

    if call.args.len() != expected_args {
        return Err(format!(
            "ExternalCall::{method_name} expects {expected_args} arguments, got {}",
            call.args.len()
        ));
    }

    let target_expr = &call.args[0];
    let selector_expr = &call.args[1];

    // 1. Write selector to mem[0x80..0xA0]: selector << 224 so the 4 bytes sit at 0x80..0x84.
    compile_expr(b, selector_expr, layout, args, locals, externals)?;
    b.emit_push_u8(0xE0); // 224
    b.emit(Opcode::LeftShift);
    b.emit_push_u8(0x80);
    b.emit(Opcode::MStore);

    // 2. Write each arg to mem[0x84 + i*32].
    for i in 0..arg_count {
        let arg_expr = &call.args[2 + i];
        compile_expr(b, arg_expr, layout, args, locals, externals)?;
        let offset: usize = 0x84 + i * 32;
        b.emit_push_usize(offset);
        b.emit(Opcode::MStore);
    }

    let calldata_size: usize = 4 + arg_count * 32;
    let ret_size: usize = if has_return { 32 } else { 0 };

    // 3. Push CALL arguments (EVM pops: gas, addr, value, argsOffset, argsSize, retOffset, retSize)
    //    so we push in reverse order.
    b.emit_push_usize(ret_size);      // retSize
    b.emit_push_u8(0x00);             // retOffset
    b.emit_push_usize(calldata_size); // argsSize
    b.emit_push_u8(0x80);             // argsOffset
    b.emit(Opcode::Push0);            // value = 0
    // 5 extra items on stack above locals — adjust DUP depth for target_expr
    emit_compile_expr_with_offset(b, target_expr, layout, args, locals, 5, externals)?;
    b.emit(Opcode::Gas);              // gas = all remaining gas
    b.emit(Opcode::Call);             // → pushes 1 (success) or 0 (failure)

    // 4. Check success; revert with returndata on failure.
    let success_label = b.create_label();
    b.emit_jumpi(success_label); // jump if success != 0
    // Failure path: bubble up returndata.
    b.emit(Opcode::ReturnDataSize);
    b.emit(Opcode::Push0);
    b.emit(Opcode::Push0);
    b.emit(Opcode::ReturnDataCopy);
    b.emit(Opcode::ReturnDataSize);
    b.emit(Opcode::Push0);
    b.emit(Opcode::Revert);

    b.place_label(success_label);

    // 5. If the call has a return value, load it from mem[0x00].
    if has_return {
        b.emit_push_u8(0x00);
        b.emit(Opcode::MLoad);
    }

    Ok(())
}

/// Compile a typed cross-contract call `ContractName::method(target, arg0, arg1, ...)` using
/// a looked-up selector from the external ABI.
fn compile_typed_external_call(
    b: &mut BytecodeBuilder,
    call: &syn::ExprCall,
    func_abi: &FunctionAbi,
    layout: &StorageLayout,
    args: &[Arg],
    locals: &mut Vec<String>,
    externals: &ExternalAbis,
) -> Result<(), String> {
    let has_return = !func_abi.outputs.is_empty();
    let arg_count = func_abi.inputs.len();

    // call.args[0] = target address, call.args[1..] = method args
    if call.args.len() != arg_count + 1 {
        return Err(format!(
            "{}() expects {} args + target, got {}",
            func_abi.name,
            arg_count,
            call.args.len()
        ));
    }

    let target_expr = &call.args[0];
    let selector_u32 = u32::from_be_bytes(func_abi.selector);

    // Write selector to mem[0x80]: selector << 224
    b.emit_push_u32(selector_u32);
    b.emit_push_u8(0xE0);
    b.emit(Opcode::LeftShift);
    b.emit_push_u8(0x80);
    b.emit(Opcode::MStore);

    // Write each method arg to mem[0x84 + i*32]
    for i in 0..arg_count {
        let arg_expr = &call.args[1 + i]; // skip target
        compile_expr(b, arg_expr, layout, args, locals, externals)?;
        let offset = 0x84 + i * 32;
        b.emit_push_usize(offset);
        b.emit(Opcode::MStore);
    }

    let calldata_size = 4 + arg_count * 32;
    let ret_size: usize = if has_return { 32 } else { 0 };

    // Push CALL args (EVM pops: gas, addr, value, argsOffset, argsSize, retOffset, retSize)
    b.emit_push_usize(ret_size);      // retSize
    b.emit_push_u8(0x00);             // retOffset
    b.emit_push_usize(calldata_size); // argsSize
    b.emit_push_u8(0x80);             // argsOffset
    b.emit(Opcode::Push0);            // value = 0
    // 5 extra items on stack above locals — adjust DUP depth for target_expr
    emit_compile_expr_with_offset(b, target_expr, layout, args, locals, 5, externals)?;
    b.emit(Opcode::Gas);
    b.emit(Opcode::Call);

    // Check success; revert with returndata on failure
    let success_label = b.create_label();
    b.emit_jumpi(success_label);
    b.emit(Opcode::ReturnDataSize);
    b.emit(Opcode::Push0);
    b.emit(Opcode::Push0);
    b.emit(Opcode::ReturnDataCopy);
    b.emit(Opcode::ReturnDataSize);
    b.emit(Opcode::Push0);
    b.emit(Opcode::Revert);
    b.place_label(success_label);

    if has_return {
        b.emit_push_u8(0x00);
        b.emit(Opcode::MLoad);
    }

    Ok(())
}

// ---- Creation code builder ----

/// Returns the byte-size of `emit_push_usize(val)`.
fn push_usize_size(val: usize) -> usize {
    if val == 0 {
        1 // PUSH0
    } else if val < 0x100 {
        2 // PUSH1 + 1 byte
    } else if val < 0x10000 {
        3 // PUSH2 + 2 bytes
    } else if val < 0x1000000 {
        4 // PUSH3 + 3 bytes
    } else {
        5 // PUSH4 + 4 bytes
    }
}

fn compile_creation_code(
    runtime_bytecode: &[u8],
    constructor_args: &[Arg],
    _layout: &StorageLayout,
) -> Result<Vec<u8>, String> {
    let num_args = constructor_args.len();
    let total_args_size = num_args * 32;
    let runtime_len = runtime_bytecode.len();

    let mut b = BytecodeBuilder::new();

    if num_args > 0 {
        // Copy all constructor args to memory at 0x80.
        // Args are ABI-encoded and appended after the creation code by the deployer.
        // CODESIZE gives total code length including appended args.
        // Stack layout before CODECOPY: [0x80(top/destOffset), (CODESIZE-args_size)(offset), args_size(size)]
        // EVM SUB: a - b where a=TOS, b=next; to get CODESIZE-args_size we need CODESIZE on top.
        b.emit_push_usize(total_args_size); // size (stays at bottom)
        b.emit_push_usize(total_args_size); // will be subtracted from CODESIZE
        b.emit(Opcode::CodeSize); // CODESIZE on top (= a for SUB)
        b.emit(Opcode::Sub); // offset = CODESIZE - args_size
        b.emit_push_u8(0x80); // dest in memory
        b.emit(Opcode::CodeCopy);

        // Store each arg into its corresponding storage slot (by convention: arg i → slot i).
        for i in 0..num_args {
            let mem_offset = 0x80_usize + i * 32;
            b.emit_push_usize(mem_offset);
            b.emit(Opcode::MLoad); // value on stack
            b.emit_push_usize(i); // slot
            b.emit(Opcode::SStore);
        }
    }

    // Emit the "return runtime code" sequence.
    // We need to know the offset of the runtime code within the creation deployment,
    // which equals: (bytes emitted so far) + (size of the return sequence itself).
    //
    // The return sequence is:
    //   emit_push_usize(runtime_len)  [push_usize_size(runtime_len) bytes]
    //   emit_push_u16(offset as u16)  [3 bytes — always PUSH2]
    //   emit_push_u8(0x00)            [2 bytes]
    //   CODECOPY                      [1 byte]
    //   emit_push_usize(runtime_len)  [push_usize_size(runtime_len) bytes]
    //   emit_push_u8(0x00)            [2 bytes]
    //   RETURN                        [1 byte]
    //
    // Total = push_usize_size(runtime_len)*2 + 3 + 2 + 1 + 2 + 1
    //       = push_usize_size(runtime_len)*2 + 9
    //
    // We use PUSH2 for the offset so its size is always fixed at 3 bytes.
    let body_len = b.len();
    let return_seq_len = push_usize_size(runtime_len) * 2 + 9;
    let runtime_offset = body_len + return_seq_len;

    b.emit_push_usize(runtime_len);
    b.emit_push_u16(runtime_offset as u16);
    b.emit_push_u8(0x00);
    b.emit(Opcode::CodeCopy);
    b.emit_push_usize(runtime_len);
    b.emit_push_u8(0x00);
    b.emit(Opcode::Return);

    assert_eq!(
        b.len(),
        runtime_offset,
        "creation code offset mismatch: expected {runtime_offset}, got {}",
        b.len()
    );

    let mut creation = b.finalize();
    creation.extend_from_slice(runtime_bytecode);
    Ok(creation)
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_counter_contract() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Counter {
                pub value: StorageItem<U256>,
            }
        };

        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Counter {
                pub fn constructor(initial: U256) -> Self {
                    Counter { value: StorageItem::new_with_value("value", initial) }
                }
                pub fn get_value(&self) -> U256 {
                    self.value.get_value()
                }
                pub fn increment(&mut self) {
                    self.value.set(self.value.get_value() + U256::from(1))
                }
            }
        };

        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());

        let compiled = result.unwrap();
        assert_eq!(compiled.name, "Counter");
        assert_eq!(compiled.functions.len(), 2); // get_value + increment
        assert!(compiled.constructor.is_some());
        assert!(!compiled.runtime_bytecode.is_empty());
        assert!(!compiled.creation_bytecode.is_empty());

        // Runtime bytecode starts with selector dispatch:
        // PUSH1 0x00 = 0x60 0x00
        assert_eq!(compiled.runtime_bytecode[0], 0x60); // PUSH1
        assert_eq!(compiled.runtime_bytecode[1], 0x00);
        assert_eq!(compiled.runtime_bytecode[2], 0x35); // CALLDATALOAD
    }

    #[test]
    fn compile_no_constructor() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Simple {
                pub count: StorageItem<U256>,
            }
        };

        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Simple {
                pub fn get_count(&self) -> U256 {
                    self.count.get_value()
                }
            }
        };

        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());

        let compiled = result.unwrap();
        assert!(compiled.constructor.is_none());
        assert_eq!(compiled.functions.len(), 1);
        // When no constructor, emit_creation_code is used
        assert!(compiled.creation_bytecode.ends_with(&compiled.runtime_bytecode));
    }

    #[test]
    fn push_usize_size_correct() {
        assert_eq!(push_usize_size(0), 1);
        assert_eq!(push_usize_size(1), 2);
        assert_eq!(push_usize_size(255), 2);
        assert_eq!(push_usize_size(256), 3);
        assert_eq!(push_usize_size(0xffff), 3);
        assert_eq!(push_usize_size(0x10000), 4);
    }

    #[test]
    fn compile_double_map_contract() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Allowances {
                pub allowances: StorageDoubleMap<Address, Address, U256>,
            }
        };
        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Allowances {
                pub fn get_allowance(&self, owner: Address, spender: Address) -> U256 {
                    self.allowances.get(&owner, &spender)
                }
                pub fn set_allowance(&mut self, owner: Address, spender: Address, amount: U256) {
                    self.allowances.set_value(&owner, &spender, amount)
                }
            }
        };
        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());
        let compiled = result.unwrap();
        assert_eq!(compiled.functions.len(), 2);
    }

    #[test]
    fn compile_self_address_and_msg_value() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Env {
                pub dummy: StorageItem<U256>,
            }
        };
        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Env {
                pub fn get_self(&self) -> Address {
                    self_address()
                }
                pub fn get_value(&self) -> U256 {
                    msg_value()
                }
            }
        };
        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());
        let compiled = result.unwrap();
        assert_eq!(compiled.functions.len(), 2);
        // Verify ADDRESS opcode (0x30) and CALLVALUE opcode (0x34) appear in bytecode.
        assert!(
            compiled.runtime_bytecode.contains(&0x30),
            "ADDRESS opcode not found"
        );
        assert!(
            compiled.runtime_bytecode.contains(&0x34),
            "CALLVALUE opcode not found"
        );
    }

    #[test]
    fn compile_external_call_intrinsic() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Vault {
                pub token: StorageItem<U256>,
            }
        };
        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Vault {
                pub fn do_transfer(&mut self, target: U256, to: U256, amount: U256) {
                    ExternalCall::call_void_2(target, 0xa9059cbbu32, to, amount)
                }
                pub fn query_balance(&self, target: U256, account: U256) -> U256 {
                    ExternalCall::call_1(target, 0x70a08231u32, account)
                }
            }
        };
        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());
        let compiled = result.unwrap();
        assert_eq!(compiled.functions.len(), 2);
        // CALL opcode is 0xf1
        assert!(
            compiled.runtime_bytecode.contains(&0xf1),
            "CALL opcode not found in bytecode"
        );
        // GAS opcode is 0x5a
        assert!(
            compiled.runtime_bytecode.contains(&0x5a),
            "GAS opcode not found in bytecode"
        );
    }

    #[test]
    fn compile_erc20_patterns() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Erc20 {
                pub total_supply: StorageItem<U256>,
                pub balances: StorageMap<Address, U256>,
            }
        };

        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Erc20 {
                pub fn balance_of(&self, account: Address) -> U256 {
                    self.balances.get(&account)
                }
                pub fn transfer(&mut self, to: Address, amount: U256) {
                    let sender = msg_sender();
                    let sender_bal = self.balances.get(&sender);
                    if sender_bal < amount {
                        panic!("insufficient balance");
                    }
                    self.balances.set_value(&sender, sender_bal - amount);
                    self.balances.set_value(&to, self.balances.get(&to) + amount);
                }
            }
        };

        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());

        let compiled = result.unwrap();
        assert_eq!(compiled.functions.len(), 2);
        assert!(!compiled.runtime_bytecode.is_empty());
    }

    #[test]
    fn compile_event_emission() {
        let contract_struct: syn::ItemStruct = syn::parse_quote! {
            pub struct Token {
                pub total_supply: StorageItem<U256>,
                pub balances: StorageMap<Address, U256>,
            }
        };
        let contract_impl: syn::ItemImpl = syn::parse_quote! {
            impl Token {
                pub fn mint(&mut self, to: Address, amount: U256) {
                    self.balances.set_value(&to, self.balances.get(&to) + amount);
                    self.total_supply.set(self.total_supply.get_value() + amount);
                    emit_log1_raw(U256::from(0x1234u32), amount);
                }
                pub fn transfer(&mut self, from: Address, to: Address, amount: U256) {
                    self.balances.set_value(&to, self.balances.get(&to) + amount);
                    emit_log2_raw(U256::from(0xabcdu32), U256::from(0x5678u32), amount);
                }
                pub fn triple_topic(&mut self, a: U256, b: U256, c: U256) {
                    emit_log3_raw(U256::from(0x01u32), U256::from(0x02u32), U256::from(0x03u32), a);
                }
            }
        };
        let result = compile(&contract_struct, &contract_impl);
        assert!(result.is_ok(), "compilation failed: {:?}", result.err());

        let compiled = result.unwrap();
        assert_eq!(compiled.functions.len(), 3);
        assert!(!compiled.runtime_bytecode.is_empty());

        // LOG1 = 0xa1, LOG2 = 0xa2, LOG3 = 0xa3
        assert!(
            compiled.runtime_bytecode.contains(&0xa1),
            "LOG1 opcode not found in bytecode"
        );
        assert!(
            compiled.runtime_bytecode.contains(&0xa2),
            "LOG2 opcode not found in bytecode"
        );
        assert!(
            compiled.runtime_bytecode.contains(&0xa3),
            "LOG3 opcode not found in bytecode"
        );
    }
}
