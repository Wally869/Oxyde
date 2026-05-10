use crate::opcodes::Opcode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LabelId(usize);

pub struct BytecodeBuilder {
    code: Vec<u8>,
    labels: Vec<Option<usize>>,
    label_references: Vec<(usize, LabelId)>,
}

impl BytecodeBuilder {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            labels: Vec::new(),
            label_references: Vec::new(),
        }
    }

    pub fn emit(&mut self, op: Opcode) {
        self.code.push(op.to_byte());
    }

    pub fn emit_push_u8(&mut self, val: u8) {
        self.code.push(Opcode::Push(1).to_byte());
        self.code.push(val);
    }

    pub fn emit_push_u16(&mut self, val: u16) {
        self.code.push(Opcode::Push(2).to_byte());
        self.code.push((val >> 8) as u8);
        self.code.push(val as u8);
    }

    pub fn emit_push_u32(&mut self, val: u32) {
        self.code.push(Opcode::Push(4).to_byte());
        self.code.push((val >> 24) as u8);
        self.code.push((val >> 16) as u8);
        self.code.push((val >> 8) as u8);
        self.code.push(val as u8);
    }

    pub fn emit_push_u256(&mut self, val: &[u8; 32]) {
        let leading_zeros = val.iter().take_while(|&&b| b == 0).count();
        let significant = 32 - leading_zeros;

        if significant == 0 {
            self.code.push(Opcode::Push0.to_byte());
        } else {
            let width = significant as u8;
            self.code.push(Opcode::Push(width).to_byte());
            self.code.extend_from_slice(&val[leading_zeros..]);
        }
    }

    pub fn emit_push_usize(&mut self, val: usize) {
        if val == 0 {
            self.code.push(Opcode::Push0.to_byte());
        } else if val < 0x100 {
            self.emit_push_u8(val as u8);
        } else if val < 0x10000 {
            self.emit_push_u16(val as u16);
        } else if val < 0x1000000 {
            self.code.push(Opcode::Push(3).to_byte());
            self.code.push((val >> 16) as u8);
            self.code.push((val >> 8) as u8);
            self.code.push(val as u8);
        } else if val <= u32::MAX as usize {
            self.emit_push_u32(val as u32);
        } else {
            let bytes = (val as u64).to_be_bytes();
            let leading = bytes.iter().take_while(|&&b| b == 0).count();
            let width = (8 - leading) as u8;
            self.code.push(Opcode::Push(width).to_byte());
            self.code.extend_from_slice(&bytes[leading..]);
        }
    }

    pub fn create_label(&mut self) -> LabelId {
        let id = self.labels.len();
        self.labels.push(None);
        LabelId(id)
    }

    pub fn place_label(&mut self, label: LabelId) {
        let offset = self.code.len();
        self.labels[label.0] = Some(offset);
        self.emit(Opcode::JumpDest);
    }

    pub fn emit_jump(&mut self, label: LabelId) {
        self.code.push(Opcode::Push(2).to_byte());
        let ref_offset = self.code.len();
        self.label_references.push((ref_offset, label));
        self.code.push(0x00);
        self.code.push(0x00);
        self.emit(Opcode::Jump);
    }

    pub fn emit_jumpi(&mut self, label: LabelId) {
        self.code.push(Opcode::Push(2).to_byte());
        let ref_offset = self.code.len();
        self.label_references.push((ref_offset, label));
        self.code.push(0x00);
        self.code.push(0x00);
        self.emit(Opcode::JumpI);
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn finalize(mut self) -> Vec<u8> {
        for (ref_offset, label) in self.label_references {
            let target = self.labels[label.0]
                .unwrap_or_else(|| panic!("label {:?} was referenced but never placed", label));
            assert!(
                target <= 0xffff,
                "label target {target} exceeds PUSH2 range"
            );
            self.code[ref_offset] = (target >> 8) as u8;
            self.code[ref_offset + 1] = target as u8;
        }
        self.code
    }

    pub fn emit_creation_code(runtime_bytecode: &[u8]) -> Vec<u8> {
        let runtime_len = runtime_bytecode.len();

        let mut preamble = BytecodeBuilder::new();

        preamble.emit_push_usize(runtime_len);
        let preamble_len_after_first_push = preamble.len();
        let _ = preamble_len_after_first_push;

        let placeholder_offset: u8 = 0x00;
        preamble.emit_push_u8(placeholder_offset);
        preamble.emit_push_u8(0x00);
        preamble.emit(Opcode::CodeCopy);

        preamble.emit_push_usize(runtime_len);
        preamble.emit_push_u8(0x00);
        preamble.emit(Opcode::Return);

        let preamble_len = preamble.len();

        let mut builder = BytecodeBuilder::new();
        builder.emit_push_usize(runtime_len);
        builder.emit_push_u8(preamble_len as u8);
        builder.emit_push_u8(0x00);
        builder.emit(Opcode::CodeCopy);

        builder.emit_push_usize(runtime_len);
        builder.emit_push_u8(0x00);
        builder.emit(Opcode::Return);

        assert_eq!(builder.len(), preamble_len, "preamble size mismatch");

        let mut result = builder.finalize();
        result.extend_from_slice(runtime_bytecode);
        result
    }
}

impl Default for BytecodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcodes::Opcode;

    #[test]
    fn emit_push_u8_correct_bytes() {
        let mut b = BytecodeBuilder::new();
        b.emit_push_u8(0xab);
        let code = b.finalize();
        assert_eq!(code, vec![0x60, 0xab]);
        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push(1)));
    }

    #[test]
    fn emit_push_u16_correct_bytes() {
        let mut b = BytecodeBuilder::new();
        b.emit_push_u16(0x1234);
        let code = b.finalize();
        assert_eq!(code, vec![0x61, 0x12, 0x34]);
        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push(2)));
    }

    #[test]
    fn emit_push_u32_correct_bytes() {
        let mut b = BytecodeBuilder::new();
        b.emit_push_u32(0x12345678);
        let code = b.finalize();
        assert_eq!(code, vec![0x63, 0x12, 0x34, 0x56, 0x78]);
        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push(4)));
    }

    #[test]
    fn emit_push_u256_minimal_width() {
        let mut val = [0u8; 32];
        val[31] = 0x42;

        let mut b = BytecodeBuilder::new();
        b.emit_push_u256(&val);
        let code = b.finalize();
        assert_eq!(code, vec![0x60, 0x42]);
        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push(1)));
    }

    #[test]
    fn emit_push_u256_full_32_bytes() {
        let val = [0xffu8; 32];

        let mut b = BytecodeBuilder::new();
        b.emit_push_u256(&val);
        let code = b.finalize();
        assert_eq!(code.len(), 33);
        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push(32)));
        assert!(code[1..].iter().all(|&x| x == 0xff));
    }

    #[test]
    fn emit_push_u256_zero_uses_push0() {
        let val = [0u8; 32];

        let mut b = BytecodeBuilder::new();
        b.emit_push_u256(&val);
        let code = b.finalize();
        assert_eq!(code, vec![0x5f]);
        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push0));
    }

    #[test]
    fn emit_push_usize_minimal_width() {
        let mut b = BytecodeBuilder::new();
        b.emit_push_usize(0);
        let code_zero = b.finalize();
        assert_eq!(code_zero, vec![0x5f]);

        let mut b = BytecodeBuilder::new();
        b.emit_push_usize(255);
        let code_u8 = b.finalize();
        assert_eq!(code_u8, vec![0x60, 0xff]);

        let mut b = BytecodeBuilder::new();
        b.emit_push_usize(256);
        let code_u16 = b.finalize();
        assert_eq!(code_u16, vec![0x61, 0x01, 0x00]);

        let mut b = BytecodeBuilder::new();
        b.emit_push_usize(0x10000);
        let code_u24 = b.finalize();
        assert_eq!(code_u24, vec![0x62, 0x01, 0x00, 0x00]);

        let mut b = BytecodeBuilder::new();
        b.emit_push_usize(0x1000000);
        let code_u32 = b.finalize();
        assert_eq!(code_u32, vec![0x63, 0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn label_forward_jump_patches_correctly() {
        let mut b = BytecodeBuilder::new();

        let label = b.create_label();
        b.emit_jump(label);
        b.place_label(label);

        let code = b.finalize();

        assert_eq!(Opcode::from_byte(code[0]), Some(Opcode::Push(2)));
        let target = ((code[1] as usize) << 8) | (code[2] as usize);
        assert_eq!(Opcode::from_byte(code[3]), Some(Opcode::Jump));
        assert_eq!(Opcode::from_byte(code[4]), Some(Opcode::JumpDest));
        assert_eq!(target, 4);
    }

    #[test]
    fn jumpi_patches_correctly() {
        let mut b = BytecodeBuilder::new();

        let label = b.create_label();
        b.emit_push_u8(0x01);
        b.emit_jumpi(label);
        b.place_label(label);

        let code = b.finalize();

        let push2_pos = 2;
        assert_eq!(Opcode::from_byte(code[push2_pos]), Some(Opcode::Push(2)));
        let target = ((code[push2_pos + 1] as usize) << 8) | (code[push2_pos + 2] as usize);
        assert_eq!(Opcode::from_byte(code[push2_pos + 3]), Some(Opcode::JumpI));
        assert_eq!(Opcode::from_byte(code[push2_pos + 4]), Some(Opcode::JumpDest));
        assert_eq!(target, push2_pos + 4);
    }

    #[test]
    #[should_panic]
    fn finalize_panics_on_unplaced_label() {
        let mut b = BytecodeBuilder::new();
        let label = b.create_label();
        b.emit_jump(label);
        b.finalize();
    }

    #[test]
    fn emit_creation_code_structure() {
        let runtime = vec![0x60, 0x00, 0x56];
        let creation = BytecodeBuilder::emit_creation_code(&runtime);

        assert!(creation.ends_with(&runtime));

        let preamble_len = creation.len() - runtime.len();
        assert!(preamble_len > 0);

        let last_preamble_byte = creation[preamble_len - 1];
        assert_eq!(Opcode::from_byte(last_preamble_byte), Some(Opcode::Return));

        assert!(creation[..preamble_len].contains(&Opcode::CodeCopy.to_byte()));

        assert!(creation[..preamble_len].contains(&Opcode::Return.to_byte()));

        let runtime_len_byte = runtime.len() as u8;
        assert!(creation[..preamble_len].contains(&runtime_len_byte));

        let offset_byte = preamble_len as u8;
        assert!(creation[..preamble_len].contains(&offset_byte));
    }

    #[test]
    fn opcodes_roundtrip_via_emit() {
        let mut b = BytecodeBuilder::new();
        let ops = [
            Opcode::Add,
            Opcode::SLoad,
            Opcode::SStore,
            Opcode::Return,
            Opcode::JumpDest,
        ];
        for op in &ops {
            b.emit(*op);
        }
        let code = b.finalize();
        for (i, op) in ops.iter().enumerate() {
            assert_eq!(Opcode::from_byte(code[i]), Some(*op));
        }
    }
}
