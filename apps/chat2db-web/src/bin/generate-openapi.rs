use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &chat2db_web::openapi())?;
    writeln!(output)?;
    Ok(())
}
