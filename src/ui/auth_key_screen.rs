use iced::{
    button, text_input, Align, Button, Clipboard, Color, Column, Command, Element,
    HorizontalAlignment, Length, Row, Space, Text, TextInput, VerticalAlignment,
};
use tracing::error;

use crate::ui::style::normal_text;
use crate::ui::{Message, Screen};

#[derive(Default, Debug)]
pub struct AuthKeyScreen {
    input_state: text_input::State,
    input_value: String,
    button_state: button::State,
}

#[derive(Clone, Debug)]
pub enum AuthKeyMessage {
    InputChanged(String),
    Save,
}

impl AuthKeyScreen {
    pub fn update(
        &mut self,
        message: AuthKeyMessage,
        _clipboard: &mut Clipboard,
    ) -> Command<Message> {
        use AuthKeyMessage::*;
        match message {
            InputChanged(s) => {
                self.input_value = s;
            }
            Save => {
                let s = self.input_value.trim().to_string();
                return Command::perform(async { s }, Message::AuthKeySave);
            }
        }
        Command::none()
    }

    pub fn view(&mut self) -> Element<AuthKeyMessage> {
        let message = normal_text("Please enter your Authentication Key below.")
            .horizontal_alignment(HorizontalAlignment::Center);

        let input = TextInput::new(
            &mut self.input_state,
            "",
            &self.input_value,
            AuthKeyMessage::InputChanged,
        )
        .padding(10)
        .size(20);

        let button = Button::new(
            &mut self.button_state,
            Text::new("Save")
                .height(Length::Fill)
                .vertical_alignment(VerticalAlignment::Center),
        )
        .on_press(AuthKeyMessage::Save);

        Column::new()
            .align_items(Align::Center)
            .push(Space::new(Length::Fill, Length::Units(50)))
            .push(message)
            .push(Space::new(Length::Fill, Length::Units(10)))
            .push(
                Row::new()
                    .max_width(250)
                    .height(Length::Units(40))
                    .push(input)
                    .push(button.height(Length::Fill)),
            )
            .into()
    }

    fn background_color(&self) -> Color {
        Color::from_rgb(0.168, 0.243, 0.313).into()
    }
}