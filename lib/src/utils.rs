/// Returns the lower 8 bits of a 16-bit word
pub fn lo(value: u16) -> u8 {
    value as u8
}

/// Returns the higher 8 bits of a 16-bit word
pub fn hi(value: u16) -> u8 {
    (value >> 8) as u8
}

/// Returns a 16-bit word from given lower and higher 8 bits
pub fn word(lo: u8, hi: u8) -> u16 {
    u16::from_le_bytes([lo, hi])
}

/// Bits are indexed right to left starting from 0
pub fn get_bit(value: u8, bit_index: usize) -> bool {
    (value >> bit_index) & 1 == 1
}

/// Bits are indexed right to left starting from 0
pub fn set_bit(value: u8, bit_index: usize, bit: bool) -> u8 {
    if bit {
        value | (1 << bit_index)
    } else {
        value & !(1 << bit_index)
    }
}

/// Adds a and b\
/// Returns (result, half_carry, carry)
pub fn add_bytes(a: u8, b: u8) -> (u8, bool, bool) {
    let (result, carry) = a.overflowing_add(b);

    // Check half carry (bit 3)
    let h_carry = ((a & 0x0F) + (b & 0x0F)) > 0x0F;

    (result, h_carry, carry)
}

/// Adds a, b and the given carry\
/// Returns (result, half_carry, carry)
pub fn add_bytes_carry(a: u8, b: u8, carry: bool) -> (u8, bool, bool) {
    let (result, carry1) = a.overflowing_add(b);
    let (result, carry2) = result.overflowing_add(carry as u8);

    // Check half-carry (bit 3)
    let h_carry = ((a & 0x0F) + (b & 0x0F) + (carry as u8)) > 0x0F;

    (result, h_carry, carry1 || carry2)
}

/// Adds a and b\
/// Returns (result, half_carry, carry)
pub fn add_words(a: u16, b: u16) -> (u16, bool, bool) {
    let (result, carry) = a.overflowing_add(b);

    // Check half-carry (bit 11)
    let h_carry = ((a & 0x0FFF) + (b & 0x0FFF)) > 0x0FFF;

    (result, h_carry, carry)
}

/// Adds the signed byte b to the unsigned word a returning (result, half_carry, carry)
/// half_carry and carry are based on the lower 8 bits onlyd
pub fn add_word_signed_byte(a: u16, b: i8) -> (u16, bool, bool) {
    let result = a.wrapping_add(b as i16 as u16);

    // For flag calculation, treat as 8-bit addition of lower byte of SP and the immediate
    let lower_a = (a & 0xFF) as u8;
    let b_u8 = b as u8; // Preserves two's complement for negative values

    // Half-carry occurs if there's a carry from bit 3
    let h_carry = (lower_a & 0x0F) + (b_u8 & 0x0F) > 0x0F;

    // Carry occurs if there's a carry from bit 7
    let carry = lower_a.overflowing_add(b_u8).1;

    (result, h_carry, carry)
}

/// Subtracts b from a\
/// Returns (result, half_carry, carry)
pub fn sub_bytes(a: u8, b: u8) -> (u8, bool, bool) {
    let (result, carry) = a.overflowing_sub(b);

    // Check half-carry (bit 3)
    let h_carry = (a & 0x0F) < (b & 0x0F);

    (result, h_carry, carry)
}

/// Subtracts b and the given carry from a\
/// Returns (result, half_carry, carry)
pub fn sub_bytes_carry(a: u8, b: u8, carry: bool) -> (u8, bool, bool) {
    let (result1, carry1) = a.overflowing_sub(b);
    let (result, carry2) = result1.overflowing_sub(carry as u8);

    // Check half-carry (bit 3)
    let h_carry = (a & 0x0F) < ((b & 0x0F) + (carry as u8));

    (result, h_carry, carry1 || carry2)
}

/// Rotates the value left by 1, returning (result, carry)
/// ```text
/// ┏━ Carry ━┓   ┏━━━━━━ u8 ━━━━━━━┓
/// ┃    C   ←╂─┬─╂─ b7 ← ... ← b0 ←╂─┐
/// ┗━━━━━━━━━┛ │ ┗━━━━━━━━━━━━━━━━━┛ │
///             └─────────────────────┘
/// ```
pub fn rotate_left_get_carry(value: u8) -> (u8, bool) {
    let result = value.rotate_left(1);
    let carry = get_bit(value, 7);
    (result, carry)
}

/// Rotates the value right by 1, returning (result, carry)
/// ```text
///   ┏━━━━━━━ u8 ━━━━━━┓   ┏━ Carry ━┓
/// ┌─╂→ b7 → ... → b0 ─╂─┬─╂→   C    ┃
/// │ ┗━━━━━━━━━━━━━━━━━┛ │ ┗━━━━━━━━━┛
/// └─────────────────────┘
/// ```
pub fn rotate_right_get_carry(value: u8) -> (u8, bool) {
    let result = value.rotate_right(1);
    let carry = get_bit(value, 0);
    (result, carry)
}

/// Rotates the value right by 1 THROUGH the given carry, returning (result, new_carry)
/// ```text
///   ┏━━━━━━━ u8 ━━━━━━┓ ┏━ Carry ━┓
/// ┌─╂→ b7 → ... → b0 ─╂─╂→   C   ─╂─┐
/// │ ┗━━━━━━━━━━━━━━━━━┛ ┗━━━━━━━━━┛ │
/// └─────────────────────────────────┘
/// ```
pub fn rotate_right_through_carry(value: u8, carry: bool) -> (u8, bool) {
    let new_carry = get_bit(value, 0);
    let result = set_bit(value >> 1, 7, carry);
    (result, new_carry)
}

/// Rotates the value left by 1 THROUGH the given carry, returning (result, new_carry)
/// ```text
///   ┏━ Carry ━┓ ┏━━━━━━ u8 ━━━━━━━┓
/// ┌─╂─   C   ←╂─╂─ b7 ← ... ← b0 ←╂─┐
/// │ ┗━━━━━━━━━┛ ┗━━━━━━━━━━━━━━━━━┛ │
/// └─────────────────────────────────┘
/// ```
pub fn rotate_left_through_carry(value: u8, carry: bool) -> (u8, bool) {
    let new_carry = get_bit(value, 7);
    let result = set_bit(value << 1, 0, carry);
    (result, new_carry)
}
