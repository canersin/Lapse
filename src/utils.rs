pub fn load_icon() -> (Vec<u8>, u32, u32) {
    let mut image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .unwrap()
        .into_rgba8();
    let (width, height) = image.dimensions();

    let size = width.min(height);
    let x = (width - size) / 2;
    let y = (height - size) / 2;

    let cropped = image::imageops::crop(&mut image, x, y, size, size).to_image();
    (cropped.into_raw(), size, size)
}
