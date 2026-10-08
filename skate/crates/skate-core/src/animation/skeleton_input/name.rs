use crate::animation::output::attributes::AttributeName;

pub const RADIX_POWERS: [u32; 6] = [79_235_168, 2_085_136, 54_872, 1_444, 38, 1];

/// Encode up to30 bytes, stopping at NUL. Native accepts signed bytes and does
/// not validate its alphabet. Preserve its case folding, punctuation behavior,
/// wrapping word arithmetic and truncation; do not substitute UTF-8 casefold.
pub const fn encode(text: &[u8]) -> AttributeName {
    let mut words = [0u32; 5];
    let mut position = 0;
    while position < 30 && position < text.len() && text[position] != 0 {
        let mut weight = RADIX_POWERS[0]; // 38^5, exactly the native first weight.
        let word = position / 6;
        let mut count = 0;
        while count < 6 && position < text.len() && text[position] != 0 {
            let byte = text[position] as i8 as i32;
            let digit = if byte >= 97 {
                byte - 86
            } else if byte > 90 {
                byte - 58
            } else if byte > 57 {
                byte - 54
            } else {
                byte - 47
            };
            words[word] = words[word].wrapping_add((digit as u32).wrapping_mul(weight));
            weight /= 38;
            count += 1;
            position += 1;
        }
    }
    AttributeName(words)
}
