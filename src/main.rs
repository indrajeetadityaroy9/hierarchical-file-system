use mathnote::App;
use ratatui_image::picker::Picker;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        App::new(picker).run(terminal)
    })?;
    Ok(())
}
