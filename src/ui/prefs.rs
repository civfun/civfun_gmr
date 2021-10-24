use iced::{button, Button, Element};

use crate::ui::style::{action_button, done_icon, ActionButtonStyle, ButtonView, NORMAL_ICON_SIZE};
use crate::ui::{Message, Screen};

#[derive(Default, Debug)]
pub struct Prefs {
    close_settings_button_state: button::State,
    open_folder_button_state: button::State,
}

impl Prefs {
    pub fn view(&mut self) -> Element<Message> {
        let close_button = action_button(
            ButtonView::TextIcon("Done", done_icon(NORMAL_ICON_SIZE)),
            Message::SetScreen(Screen::NothingYet),
            &mut self.close_settings_button_state,
        );

        close_button.into()
    }
}
