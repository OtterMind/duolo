//! 单行输入框：新建任务、添加项目时使用。

#[derive(Debug, Default, Clone)]
pub struct LineInput {
    text: String,
    /// 光标位置（字符下标）。
    cursor: usize,
}

impl LineInput {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// 光标之前的文字，用来计算光标在屏幕上的位置。
    pub fn before_cursor(&self) -> &str {
        &self.text[..self.byte_index(self.cursor)]
    }

    pub fn insert_str(&mut self, s: &str) {
        let at = self.byte_index(self.cursor);
        // 单行输入：换行折叠成空格。
        let s = s.replace(['\r', '\n'], " ");
        self.text.insert_str(at, &s);
        self.cursor += s.chars().count();
    }

    pub fn insert(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut buf));
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = self.byte_index(self.cursor);
        let start = self.byte_index(self.cursor - 1);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    /// 删除光标前的整段文字（Ctrl+U）。
    pub fn clear_before_cursor(&mut self) {
        let at = self.byte_index(self.cursor);
        self.text.replace_range(..at, "");
        self.cursor = 0;
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_with_multibyte_chars() {
        let mut input = LineInput::default();
        input.insert_str("导出CSV");
        input.left();
        input.left();
        input.left();
        input.insert(' ');
        assert_eq!(input.text(), "导出 CSV");
        assert_eq!(input.before_cursor(), "导出 ");
        input.backspace();
        input.backspace();
        assert_eq!(input.text(), "导CSV");
        input.end();
        input.insert_str("\n订单");
        assert_eq!(input.text(), "导CSV 订单");
        input.home();
        input.delete();
        assert_eq!(input.text(), "CSV 订单");
    }
}
