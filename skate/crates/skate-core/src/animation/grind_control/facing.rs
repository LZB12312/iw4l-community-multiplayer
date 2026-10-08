/// n is Motion+80 deck velocity, b is Basic+128 effective board forward.
/// ground_flag is the historical API name for Motion273 (processed bit20);
/// mirrored is LIVE ISkaterAnim virtual28. Ground80 is a different vector.
pub fn backwards(n: [f32; 3], b: [f32; 3], ground_flag: bool, mirrored: bool) -> bool {
    let side = (-n[0]).mul_add(b[2], n[2] * b[0]);
    // Native fcmpu/ble falls through on unordered as well as strictly positive.
    let positive_or_unordered = !(side <= 0.0);
    positive_or_unordered ^ ground_flag ^ mirrored
}
