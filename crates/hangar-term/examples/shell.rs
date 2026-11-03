use iced::widget::text;

fn main() -> iced::Result {
    let _ = portable_pty::PtySize::default();
    iced::run(|_: &mut (), _: ()| {}, |_: &()| text("x"))
}
