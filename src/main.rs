use std::io;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use mathnote::App;
use ratatui_image::picker::Picker;

struct TerminalFeatures;

impl TerminalFeatures {
    fn enable() -> io::Result<Self> {
        execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
        Ok(Self)
    }
}

impl Drop for TerminalFeatures {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture);
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        let _terminal_features = TerminalFeatures::enable()?;
        App::new(picker).run(terminal)
    })?;
    Ok(())
}
