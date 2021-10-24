use iced::{
    button, Align, Application, Button, Color, Column, Container, Element, Font,
    HorizontalAlignment, Length, Row, Space, Text, VerticalAlignment,
};

use crate::ui::Message;
use crate::TITLE;

pub const ROW_HEIGHT: u16 = 40;
pub const NORMAL_ICON_SIZE: u16 = 20;

pub const RELAXED_PADDING: u16 = 20;

const FA_SOLID_ICONS: Font = Font::External {
    name: "FA Solid Icons",
    bytes: include_bytes!("../../fonts/fa-solid-900.ttf"),
};

const FA_BRANDS_ICONS: Font = Font::External {
    name: "FA Brand Icons",
    bytes: include_bytes!("../../fonts/fa-brands-400.ttf"),
};

pub fn centered_column<'a, M>() -> Column<'a, M> {
    Column::new()
        .width(Length::Fill)
        .align_items(Align::Center)
        .spacing(RELAXED_PADDING)
}

pub fn vertically_centered_content<'a, M, E>(e: E) -> Container<'a, M>
where
    E: Into<Element<'a, M>>,
{
    Container::new(e.into())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Align::Center)
}

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

pub enum ButtonView<'a> {
    Text(&'a str),
    Icon(Text),
    TextIcon(&'a str, Text),
}

impl<'a> ButtonView<'a> {
    fn parts(self) -> (Option<&'a str>, Option<Text>) {
        (
            match self {
                ButtonView::Text(t) => Some(t),
                ButtonView::Icon(_) => None,
                ButtonView::TextIcon(t, _) => Some(t),
            },
            match self {
                ButtonView::Text(_) => None,
                ButtonView::Icon(i) => Some(i),
                ButtonView::TextIcon(_, i) => Some(i),
            },
        )
    }
}

fn button_row<'a, M: 'a>(view: ButtonView) -> Row<'a, M> {
    let mut row: Row<M> = Row::new().height(Length::Units(ROW_HEIGHT));
    let (text, icon) = view.parts();
    if let Some(icon) = icon {
        row = row.push(button_side_pad()).push(icon);
    }
    if let Some(text) = text {
        row = row.push(button_side_pad()).push(
            normal_text(text)
                .vertical_alignment(VerticalAlignment::Center)
                .height(Length::Fill),
        );
    }
    row.push(button_side_pad())
}

pub fn action_button<'a, M: 'a>(
    view: ButtonView,
    message: M,
    state: &'a mut button::State,
) -> Button<'a, M>
where
    M: Clone,
{
    Button::new(state, button_row(view))
        .on_press(message)
        .style(ActionButtonStyle)
        .into()
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

fn black() -> Color {
    Color::BLACK
}

fn black_50alpha() -> Color {
    Color::new(0.0, 0.0, 0.0, 0.5)
}

fn black_25alpha() -> Color {
    Color::new(0.0, 0.0, 0.0, 0.25)
}

fn grey_50alpha() -> Color {
    Color::new(0.5, 0.5, 0.5, 0.5)
}

pub fn background_color() -> Color {
    Color::from_rgb(0.168, 0.243, 0.313)
}

pub fn title_text(s: &str) -> Text {
    Text::new(s).color(text_colour()).size(40)
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
