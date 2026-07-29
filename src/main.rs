mod album_color;
mod coordinator;
mod i18n;
mod metadata;
mod player;
mod style;
mod window;

use crate::window::Window;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    cosmic::applet::run::<Window>(())?;

    Ok(())
}
