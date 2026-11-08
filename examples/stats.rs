//! Load the data, import an EFT fit from stdin and print what the engine makes of it.

use std::{io::Read, path::Path, time::Instant};

use efa::{data::Catalog, engine::Engine, fit::Fit};

fn main() {
    let dir = Path::new("data/eve");
    let t = Instant::now();
    let catalog = Catalog::load_cached(dir, Path::new("data/cache")).expect("the dump loads");
    println!(
        "catalog: {} types in {:?}",
        catalog.types.len(),
        t.elapsed()
    );
    let t = Instant::now();
    let engine = Engine::load(dir, Path::new("data/cache"), catalog.skills.clone()).unwrap();
    println!("engine: {:?}", t.elapsed());

    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).unwrap();
    let (mut fit, unknown) = Fit::from_eft(&catalog, &text).unwrap();
    for d in &mut fit.drones {
        d.active = d.count;
    }
    println!("unknown: {unknown:?}");
    let t = Instant::now();
    let stats = engine.compute(&fit);
    println!("compute: {:?}", t.elapsed());
    println!("{stats:#?}");
    let options = efa::port::eft::Options::default();
    print!("{}", efa::port::eft::export(&fit, &catalog, &options, None));
}
