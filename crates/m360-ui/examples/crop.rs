//! Dev helper: crop and upscale a screenshot. `crop in.png out.png x y w h scale`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n = |i: usize| a[i].parse::<u32>().unwrap();
    let img = image::open(&a[1]).unwrap();
    let c = img.crop_imm(n(3), n(4), n(5), n(6));
    let s = n(7);
    c.resize(c.width() * s, c.height() * s, image::imageops::FilterType::Nearest).save(&a[2]).unwrap();
}
