use iced::{
    button, Color, Element, Font, HorizontalAlignment, Length, Row, Space, Text, VerticalAlignment,
};

use crate::ui::Message;
use crate::TITLE;

const FA_SOLID_ICONS: Font = Font::External {
    name: "FA Solid Icons",
    bytes: include_bytes!("../fonts/fa-solid-900.ttf"),
};

const FA_BRANDS_ICONS: Font = Font::External {
    name: "FA Brand Icons",
    bytes: include_bytes!("../fonts/fa-brands-400.ttf"),
};

pub fn title() -> Element<'static, Message> {
    Text::new(TITLE)
        .width(Length::Fill)
        .height(Length::Shrink)
        .size(30)
        .color(text_colour())
        .horizontal_alignment(HorizontalAlignment::Left)
        .vertical_alignment(VerticalAlignment::Top)
        .into()
}

fn button_side_pad() -> Space {
    Space::new(Length::Units(10), Length::Units(24))
}

pub fn button_row(icon: Option<Text>, text: Option<&str>) -> Row<Message> {
    let mut row: Row<Message> = Row::new();
    if let Some(icon) = icon {
        row = row.push(button_side_pad()).push(icon);
    }
    if let Some(text) = text {
        row = row.push(button_side_pad()).push(
            Text::new(text)
                .vertical_alignment(VerticalAlignment::Center)
                .height(Length::Fill),
        );
    }
    row.push(button_side_pad())
}

fn icon(font: Font, unicode: char, size: u16) -> Text {
    Text::new(&unicode.to_string())
        .font(font)
        .width(Length::Units(size))
        .height(Length::Fill)
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .color(Color::WHITE)
        .size(size)
}

pub fn cog_icon(size: u16) -> Text {
    icon(FA_SOLID_ICONS, '', size)
}

pub fn steam_icon(size: u16) -> Text {
    icon(FA_BRANDS_ICONS, '', size)
}

pub fn done_icon(size: u16) -> Text {
    icon(FA_SOLID_ICONS, '', size)
}

fn text_colour() -> Color {
    Color::from_rgb(0.9, 0.9, 1.0)
}

pub fn black() -> Color {
    Color::BLACK
}

pub fn black_50alpha() -> Color {
    Color::new(0.0, 0.0, 0.0, 0.5)
}

pub fn black_25alpha() -> Color {
    Color::new(0.0, 0.0, 0.0, 0.25)
}

pub fn grey_50alpha() -> Color {
    Color::new(0.5, 0.5, 0.5, 0.5)
}

pub fn background_color() -> Color {
    Color::from_rgb(0.168, 0.243, 0.313)
}

pub fn normal_text(s: &str) -> Text {
    Text::new(s).color(text_colour())
}

pub struct ActionButtonStyle;

impl ActionButtonStyle {
    fn base() -> button::Style {
        button::Style {
            background: Some(black_25alpha().into()),
            text_color: Color::WHITE,
            ..Default::default()
        }
    }
}

impl button::StyleSheet for ActionButtonStyle {
    fn active(&self) -> button::Style {
        Self::base()
    }

    fn hovered(&self) -> button::Style {
        button::Style {
            background: Some(black().into()),
            ..Self::base()
        }
    }

    fn pressed(&self) -> button::Style {
        button::Style {
            background: Some(black_50alpha().into()),
            ..Self::base()
        }
    }

    fn disabled(&self) -> button::Style {
        button::Style {
            background: Some(grey_50alpha().into()),
            ..Self::base()
        }
    }
}
