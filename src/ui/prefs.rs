use iced::{button, Button, Element};

use crate::ui::style::{button_row, done_icon, ActionButtonStyle};
use crate::ui::Message;

#[derive(Default, Debug)]
pub struct Prefs {
    close_settings_button_state: button::State,
    open_folder_button_state: button::State,
}

impl Prefs {
    pub fn view(&mut self) -> Element<Message> {
        let close_button = Button::new(
            &mut self.close_settings_button_state,
            button_row(Some(done_icon(20)), Some("Done")),
        )
        .on_press(Message::SetSettingsVisibility(false))
        .style(ActionButtonStyle);

        close_button.into()
    }
}
