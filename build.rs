use std::{env, path::Path};

fn main() {
  // YOU HAVE TO SET THE FFMPEG_DIR variable
  // println!(r"cargo:rustc-link-search=C:\ffmpeg;");
  // yikes!

  // let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
  // println!(
  //   "cargo:rustc-link-search=native={}",
  //   Path::new(&dir).join("lib").display()
  // );

  // println!("cargo:rustc-link-lib=static=libx264");
}
