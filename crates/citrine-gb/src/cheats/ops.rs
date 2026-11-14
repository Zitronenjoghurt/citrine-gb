use super::CheatTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Assign,
    #[cfg(feature = "gba")]
    AssignIndirect,
    #[cfg(feature = "gba")]
    And,
    #[cfg(feature = "gba")]
    Add,
    #[cfg(feature = "gba")]
    Or,
    #[cfg(feature = "gba")]
    IfEq,
    #[cfg(feature = "gba")]
    IfNe,
    #[cfg(feature = "gba")]
    IfLt,
    #[cfg(feature = "gba")]
    IfGt,
    #[cfg(feature = "gba")]
    IfUlt,
    #[cfg(feature = "gba")]
    IfUgt,
    #[cfg(feature = "gba")]
    IfAnd,
    #[cfg(feature = "gba")]
    IfLand,
    #[cfg(feature = "gba")]
    IfNand,
    #[cfg(feature = "gba")]
    IfButton,
    #[cfg(feature = "gba")]
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Op {
    pub(crate) kind: Kind,
    pub(crate) width: u8,
    pub(crate) address: u32,
    pub(crate) operand: u32,
    pub(crate) repeat: u32,
    pub(crate) negative_repeat: u32,
    pub(crate) address_offset: i32,
    pub(crate) operand_offset: i32,
}

impl Op {
    pub(crate) fn new(kind: Kind, width: u8, address: u32, operand: u32) -> Self {
        Self {
            kind,
            width,
            address,
            operand,
            repeat: 1,
            negative_repeat: 0,
            address_offset: 0,
            operand_offset: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RomPatch {
    pub(crate) address: u32,
    pub(crate) width: u8,
    pub(crate) value: u32,
    pub(crate) check: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpList {
    pub(crate) ops: Vec<Op>,
    pub(crate) patches: Vec<RomPatch>,
}

pub(crate) fn run(ops: &[Op], target: &mut (impl CheatTarget + ?Sized)) {
    let mut else_at = 0;
    let mut end_at = 0;
    let mut i = 0;
    while i < ops.len() {
        let op = ops[i];
        let mut operand = op.operand as i32;
        let mut address = op.address;
        let mut remaining = op.repeat;
        let mut condition = true;
        let mut condition_left = 0;
        let mut negative_left = 0;
        while remaining > 0 {
            let (write, test) = match op.kind {
                Kind::Assign => (Some(operand), None),
                #[cfg(feature = "gba")]
                Kind::AssignIndirect => {
                    address = (target.cheat_read(address, 4) as i32).wrapping_add(op.address_offset)
                        as u32;
                    (Some(operand), None)
                }
                #[cfg(feature = "gba")]
                Kind::And => (Some(read(target, address, op.width) & operand), None),
                #[cfg(feature = "gba")]
                Kind::Add => (
                    Some(read(target, address, op.width).wrapping_add(operand)),
                    None,
                ),
                #[cfg(feature = "gba")]
                Kind::Or => (Some(read(target, address, op.width) | operand), None),
                #[cfg(feature = "gba")]
                Kind::IfEq => (None, Some(read(target, address, op.width) == operand)),
                #[cfg(feature = "gba")]
                Kind::IfNe => (None, Some(read(target, address, op.width) != operand)),
                #[cfg(feature = "gba")]
                Kind::IfLt => (None, Some(read(target, address, op.width) < operand)),
                #[cfg(feature = "gba")]
                Kind::IfGt => (None, Some(read(target, address, op.width) > operand)),
                #[cfg(feature = "gba")]
                Kind::IfUlt => (
                    None,
                    Some((read(target, address, op.width) as u32) < operand as u32),
                ),
                #[cfg(feature = "gba")]
                Kind::IfUgt => (
                    None,
                    Some((read(target, address, op.width) as u32) > operand as u32),
                ),
                #[cfg(feature = "gba")]
                Kind::IfAnd => (None, Some(read(target, address, op.width) & operand != 0)),
                #[cfg(feature = "gba")]
                Kind::IfLand => (
                    None,
                    Some(read(target, address, op.width) != 0 && operand != 0),
                ),
                #[cfg(feature = "gba")]
                Kind::IfNand => (None, Some(read(target, address, op.width) & operand == 0)),
                #[cfg(feature = "gba")]
                Kind::IfButton | Kind::Never => (None, Some(false)),
            };
            if let Some(test) = test {
                condition = test;
                condition_left = op.repeat as usize;
                negative_left = op.negative_repeat as usize;
                remaining = 1;
            }
            if let Some(value) = write {
                target.cheat_write(address, op.width, value as u32);
            }
            address = address.wrapping_add(op.address_offset as u32);
            operand = operand.wrapping_add(op.operand_offset);
            remaining -= 1;
        }
        if else_at != 0 && i == else_at {
            i = end_at;
            end_at = 0;
        }
        if condition_left > 0 && !condition {
            i += condition_left;
        } else if negative_left > 0 {
            else_at = i + condition_left;
            end_at = else_at + negative_left;
        }
        i += 1;
    }
}

#[cfg(feature = "gba")]
fn read(target: &mut (impl CheatTarget + ?Sized), address: u32, width: u8) -> i32 {
    target.cheat_read(address, width) as i32
}
