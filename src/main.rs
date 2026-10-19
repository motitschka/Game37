use game37::{config, systems};

fn main() {
    let path = config::config_path();
    match config::load(&path) {
        Ok(Some(config)) => println!("Games Root: {}", config.games_root.display()),
        Ok(None) => println!("No Config at {}: First-run Setup is due", path.display()),
        Err(e) => eprintln!("{e}"),
    }
    println!("{} Systems", systems::SYSTEMS.len());
}
