//! Behaviour options of a terminal.

/// The shape of the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    /// A vertical bar before the character, as in a text editor.
    Beam,
    Underline,
}

/// What a program may do with the clipboard through `OSC 52`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Osc52 {
    Disabled,
    /// A program may put text on the clipboard but not read it. The default:
    /// agents use it to copy, and reading the clipboard from a remote program
    /// would leak whatever is on it.
    #[default]
    CopyOnly,
    PasteOnly,
    CopyAndPaste,
}

/// How a terminal behaves, as opposed to how it looks ([`Palette`] and
/// [`Font`]).
///
/// [`Palette`]: crate::Palette
/// [`Font`]: crate::Font
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Lines of history kept above the screen.
    pub scrollback: usize,
    /// The cursor the program gets until it asks for another with `DECSCUSR`.
    pub cursor_shape: CursorShape,
    /// Whether that cursor blinks.
    pub cursor_blink: bool,
    pub osc52: Osc52,
    /// Whether the kitty keyboard protocol is offered to programs. With it a
    /// program can tell Shift+Enter from Enter and Ctrl+I from Tab, which the
    /// byte stream of a classic terminal cannot express. Programs only get
    /// the encoding after asking for it, so leaving it on is safe.
    pub kitty_keyboard: bool,
    /// Whether Alt sends an escape before the key, which is how shells and
    /// editors read "Meta". When off, Alt+key sends what the layout produces.
    pub alt_sends_escape: bool,
    /// What Shift+Enter sends, if not a plain Enter. Agent CLIs read
    /// `ESC CR` (`b"\x1b\r"`) as "insert a newline instead of sending", which
    /// is what the old Hangar terminal did in agent panes; a shell pane
    /// should leave it `None`, because a shell would take the escape as the
    /// start of a key sequence. Programs that negotiated the kitty protocol
    /// get the proper `CSI 13;2u` instead and never see this.
    pub shift_enter: Option<Vec<u8>>,
    /// Whether bold text in the eight dark ANSI colours is drawn in the bright
    /// ones, which is what xterm and the old Hangar terminal did.
    pub bold_is_bright: bool,
    /// Whether releasing the mouse after selecting puts the text on the
    /// primary selection, as terminals on Linux do.
    pub copy_on_select: bool,
    /// Lines per wheel notch when scrolling the history.
    pub scroll_multiplier: f32,
    /// Characters that end a word for double-click selection.
    pub word_separators: String,
    /// Whether the bell flashes the terminal.
    pub visual_bell: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            cursor_shape: CursorShape::Block,
            cursor_blink: false,
            osc52: Osc52::CopyOnly,
            kitty_keyboard: true,
            alt_sends_escape: true,
            shift_enter: None,
            bold_is_bright: true,
            copy_on_select: false,
            scroll_multiplier: 3.0,
            word_separators: ",│`|\"' ()[]{}<>\t".to_owned(),
            visual_bell: true,
        }
    }
}
