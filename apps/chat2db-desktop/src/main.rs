#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), chat2db_desktop::DesktopError> {
    let exit_code = chat2db_desktop::run()?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
