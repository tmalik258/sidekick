//! The calculator behind Ask's instant results.
//! It turns text like "2 + 3 * 4" or "18% of 2450" into a number.

/// One piece of the input, like a number or a "+".
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent,
    Open,
    Close,
    Of,
}

/// Step 1: cut the text into tokens.
/// "2 + 30%" becomes [Num(2), Plus, Num(30), Percent].
fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c.is_ascii_digit() || c == '.' {
            let start = i;
            // "1,000": a comma between digits is a thousands separator.
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || (chars[i] == ',' && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().filter(|c| **c != ',').collect();
            let number: f64 = word.parse().ok()?;
            tokens.push(Token::Num(number));
            continue;
        }

        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "of" => tokens.push(Token::Of),
                "pi" => tokens.push(Token::Num(std::f64::consts::PI)),
                "e" => tokens.push(Token::Num(std::f64::consts::E)),
                // "5 x 3": x is a letter, so it lands here, not below.
                "x" => tokens.push(Token::Star),
                _ => return None,
            }
            continue;
        }

        let token = match c {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' | 'x' | '×' => Token::Star,
            '/' | '÷' => Token::Slash,
            '^' => Token::Caret,
            '%' => Token::Percent,
            '(' => Token::Open,
            ')' => Token::Close,
            _ => return None,
        };
        tokens.push(token);
        i += 1;
    }

    Some(tokens)
}

/// Step 2: read the tokens in the right order (brackets, then ^,
/// then * and /, then + and -) and work out the answer.
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        token
    }

    /// Lowest priority: + and -. Also "300 - 20%" means 20% of 300.
    fn sum(&mut self) -> Option<f64> {
        let mut total = self.product()?;
        loop {
            let adding = match self.peek() {
                Some(Token::Plus) => true,
                Some(Token::Minus) => false,
                _ => return Some(total),
            };
            self.next();
            let mut right = self.product()?;
            if self.peek() == Some(&Token::Percent) {
                self.next();
                right = total * right / 100.0;
            }
            if adding {
                total += right;
            } else {
                total -= right;
            }
        }
    }

    /// Middle priority: * and /.
    fn product(&mut self) -> Option<f64> {
        let mut total = self.power()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.next();
                    total *= self.power()?;
                }
                Some(Token::Slash) => {
                    self.next();
                    let right = self.power()?;
                    if right == 0.0 {
                        return None;
                    }
                    total /= right;
                }
                _ => return Some(total),
            }
        }
    }

    /// High priority: ^ (power). 2^3^2 is 2^(3^2), like on paper.
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(&Token::Caret) {
            self.next();
            let exponent = self.power()?;
            return Some(base.powf(exponent));
        }
        Some(base)
    }

    /// A minus sign in front: -5, -(2+3).
    fn unary(&mut self) -> Option<f64> {
        if self.peek() == Some(&Token::Minus) {
            self.next();
            return Some(-self.unary()?);
        }
        self.atom()
    }

    /// A number or a bracket, then "%" or "% of".
    fn atom(&mut self) -> Option<f64> {
        let mut value = match self.next()? {
            Token::Num(n) => n,
            Token::Open => {
                let inside = self.sum()?;
                if self.next()? != Token::Close {
                    return None;
                }
                inside
            }
            _ => return None,
        };

        // "18% of 2450" -> 0.18 * 2450
        if self.peek() == Some(&Token::Percent) && self.tokens.get(self.pos + 1) == Some(&Token::Of)
        {
            self.pos += 2;
            value = value / 100.0 * self.power()?;
        }
        Some(value)
    }
}

/// The one function the app calls. Gives back None when the text
/// is not a calculation, so Ask carries on as normal.
pub fn calculate(text: &str) -> Option<f64> {
    let tokens = tokenize(text)?;
    // A lone number like "42" is not worth showing as a result.
    if tokens.len() < 2 {
        return None;
    }
    let mut parser = Parser { tokens, pos: 0 };
    let answer = parser.sum()?;
    // Leftover tokens mean we did not understand all of it.
    if parser.pos != parser.tokens.len() || !answer.is_finite() {
        return None;
    }
    Some(answer)
}

/// Shows 0.1 + 0.2 as 0.3, not 0.30000000000000004.
pub fn format(answer: f64) -> String {
    let rounded = (answer * 1e10).round() / 1e10;
    let text = format!("{rounded}");
    if text == "-0" { "0".to_string() } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(text: &str) -> Option<String> {
        calculate(text).map(format)
    }

    #[test]
    fn basic_math() {
        assert_eq!(calc("2 + 3 * 4"), Some("14".into()));
        assert_eq!(calc("(2 + 3) * 4"), Some("20".into()));
        assert_eq!(calc("2 ^ 3 ^ 2"), Some("512".into()));
        assert_eq!(calc("-5 + 2"), Some("-3".into()));
        assert_eq!(calc("0.1 + 0.2"), Some("0.3".into()));
    }

    #[test]
    fn percentages() {
        assert_eq!(calc("18% of 2450"), Some("441".into()));
        assert_eq!(calc("300 - 20%"), Some("240".into()));
        assert_eq!(calc("2450 + 18%"), Some("2891".into()));
    }

    #[test]
    fn not_math() {
        assert_eq!(calc("open chrome"), None);
        assert_eq!(calc("42"), None);
        assert_eq!(calc("5 / 0"), None);
        assert_eq!(calc("2 +"), None);
        assert_eq!(calc("(2 + 3"), None);
    }
}

#[cfg(test)]
mod exercises {
    use super::*;
    fn calc(t: &str) -> Option<String> {
        calculate(t).map(format)
    }
    #[test]
    fn doc_exercises() {
        assert_eq!(calc("5 x 3"), Some("15".into()));
        assert_eq!(calc("5 X 3"), Some("15".into()));
        assert_eq!(calc("1,000 + 1"), Some("1001".into()));
        assert_eq!(calc("e * 1").map(|s| s.starts_with("2.718")), Some(true));
        assert_eq!(calc("hello world"), None);
        assert_eq!(calc("what time is it"), None);
    }
}
