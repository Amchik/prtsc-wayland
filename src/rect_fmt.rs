use std::fmt::{Display, Write as _};

use crate::points::Rectangle;

pub struct RectFmt<'a> {
    pub rect: Rectangle,
    pub fmt: &'a str,
    pub output_name: Option<&'a str>,
}

impl Display for RectFmt<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = self.fmt.split('%');

        if let Some(s) = parts.next() {
            f.write_str(s)?;
        }

        while let Some(part) = parts.next() {
            let Some(ctrl) = part.chars().next() else {
                // `part == ""` here
                f.write_char('%')?;
                if let Some(s) = parts.next() {
                    f.write_str(s)?;
                }
                continue;
            };

            let remainder = &part[ctrl.len_utf8()..];

            match ctrl {
                'x' | 'X' => write!(f, "{}{remainder}", self.rect.start.x)?,
                'y' | 'Y' => write!(f, "{}{remainder}", self.rect.start.y)?,
                'w' | 'W' => write!(f, "{}{remainder}", self.rect.width)?,
                'h' | 'H' => write!(f, "{}{remainder}", self.rect.height)?,
                'o' => write!(f, "{}{remainder}", self.output_name.unwrap_or("<unknown>"))?,
                'n' => write!(f, "\n{remainder}")?,
                _ => write!(f, "%{part}")?,
            }
        }

        Ok(())
    }
}

