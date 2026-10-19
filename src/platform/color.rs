//! Embedded RGB ICC -> sRGB. The surface remains sRGB so a color-managed
//! compositor can map it to the monitor; no guessed monitor profile is applied.
use anyhow::Result;
pub(crate) fn to_srgb(image: &mut image::DynamicImage, profile: &[u8]) -> Result<()> {
    if profile.len() > 4 * 1024 * 1024 {
        anyhow::bail!("ICC profile exceeds 4 MiB");
    }
    let context = lcms2::ThreadContext::new();
    let input = lcms2::Profile::new_icc_context(&context, profile)?;
    if input.color_space() != lcms2::ColorSpaceSignature::RgbData {
        anyhow::bail!("Unsupported non-RGB ICC profile");
    }
    let output = lcms2::Profile::new_srgb_context(&context);
    let transform = lcms2::Transform::<u8, u8, lcms2::ThreadContext>::new_context(
        &context,
        &input,
        lcms2::PixelFormat::RGB_8,
        &output,
        lcms2::PixelFormat::RGB_8,
        lcms2::Intent::RelativeColorimetric,
    )?;
    if image.width() == 0 || image.height() == 0 {
        anyhow::bail!("Empty ICC image");
    }
    // Only consume the source after profile validation succeeded.
    let mut rgba = std::mem::take(image).into_rgba8();
    // Process row buffers so ICC conversion does not allocate another full image.
    let mut row = vec![0; rgba.width() as usize * 3];
    for pixels in rgba.as_mut().chunks_exact_mut(row.len() / 3 * 4) {
        for (pixel, rgb) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(row.as_chunks_mut::<3>().0)
        {
            rgb.copy_from_slice(&pixel[..3]);
        }
        transform.transform_in_place(&mut row);
        for (pixel, rgb) in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(row.as_chunks::<3>().0)
        {
            pixel[..3].copy_from_slice(rgb);
        }
    }
    *image = image::DynamicImage::ImageRgba8(rgba);
    Ok(())
}
#[cfg(test)]
#[test]
fn srgb_round_trip_preserves_alpha_and_invalid_profiles_fail() {
    let profile = lcms2::Profile::new_srgb().icc().unwrap();
    let mut image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        3,
        2,
        image::Rgba([120, 80, 40, 37]),
    ));
    to_srgb(&mut image, &profile).unwrap();
    assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [120, 80, 40, 37]);
    let before = image.clone();
    assert!(to_srgb(&mut image, b"invalid profile").is_err());
    assert_eq!(image, before);
}
