# Build the calculator yourself

Step 1 of the calculator: math and percentages. When this works, we add units, dates and number bases the same way.

## How it works, in one picture

```
"18% of 2450"
   |  tokenize: cut the text into pieces
   v
[Num(18), Percent, Of, Num(2450)]
   |  Parser: read the pieces in the right order
   v
441
```

Two steps, like reading a sentence: first split it into words, then understand the words in order.

## Files to create

Inside the sidekick repo:

```
crates/calc/Cargo.toml
crates/calc/src/lib.rs
```

Then add `"crates/calc"` to the `members` list in the root `Cargo.toml`.

### crates/calc/Cargo.toml

```toml
[package]
name = "sidekick-calc"
version = "0.1.0"
edition = "2021"

[dependencies]
```

This is like `package.json`. No dependencies: we write it all ourselves.

### crates/calc/src/lib.rs

Type it in parts, running `cargo test -p sidekick-calc` after each part. Full file:

```rust
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
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
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
        if self.peek() == Some(&Token::Percent)
            && self.tokens.get(self.pos + 1) == Some(&Token::Of)
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
    if text == "-0" {
        "0".to_string()
    } else {
        text
    }
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
```

## The Rust you need, mapped to what you know

You know variables, loops and operators. Here is everything new in this file, in the order it shows up.

### 1. Comments

- `//` is a normal comment.
- `///` is a doc comment for the thing below it (like JSDoc).
- `//!` describes the whole file.

### 2. `enum`: a value that is one of a few kinds

```rust
enum Token {
    Num(f64),
    Plus,
    ...
}
```

In TypeScript this is like `type Token = { kind: "num", value: number } | { kind: "plus" } | ...`.
`Num(f64)` carries a number inside it; `Plus` carries nothing. `f64` is a decimal number (like JS `number`).

`#[derive(Debug, Clone, PartialEq)]` asks Rust to write three things for us: printing it (`Debug`), copying it (`Clone`), and comparing with `==` (`PartialEq`).

### 3. Functions and types

```rust
fn tokenize(text: &str) -> Option<Vec<Token>> {
```

- `fn` = `function`.
- `text: &str` = a piece of text we only read. The `&` means "borrow it, do not take it".
- `-> Option<Vec<Token>>` is the return type.
  - `Vec<Token>` is a list of tokens (like `Token[]`).
  - `Option<...>` means "maybe a value". It is either `Some(value)` or `None`. Rust has no `null`; this is how it says "might be missing".

### 4. `let` and `let mut`

```rust
let mut tokens = Vec::new();
let chars: Vec<char> = ...;
```

- `let` is like `const`: it cannot change.
- `let mut` is like `let` in JS: it can change.

`Vec::new()` makes an empty list. `tokens.push(x)` adds to it, same as JS.

### 5. `while` and `continue`

Same as JS. `i += 1` moves to the next character.

### 6. `chars[start..i]`

A slice: characters from `start` up to (not including) `i`. Like `arr.slice(start, i)`.

### 7. The `?` operator: "give up if missing"

```rust
let number: f64 = word.parse().ok()?;
```

`word.parse()` tries to read a number. If it fails, the `?` makes the whole function return `None` right there. It saves writing `if (!x) return null;` again and again. You will see `?` all over the file.

### 8. `match`: a better `switch`

```rust
match word.as_str() {
    "of" => tokens.push(Token::Of),
    "pi" => tokens.push(Token::Num(std::f64::consts::PI)),
    _ => return None,
}
```

- Each line is `pattern => what to do`.
- `_` means "anything else" (like `default:`).
- Rust makes you handle every case, so you cannot forget one.

`'*' | 'x' | '×' => Token::Star` means "any of these three".

### 9. `struct` and `impl`: data with functions

```rust
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> { ... }
    fn next(&mut self) -> Option<Token> { ... }
}
```

This is like a class: `struct` holds the data, `impl` holds the methods.

- `&self` = the method only reads (like a getter).
- `&mut self` = the method changes it (here, it moves `pos` forward).
- `usize` is a whole number that cannot be negative (used for positions).

### 10. Why `sum`, `product`, `power`, `unary`, `atom`?

This is the one clever idea in the file. Each function handles one level of priority, and each one calls the next stronger level first:

```
sum      handles + and -       (weakest)
product  handles * and /
power    handles ^
unary    handles a leading minus
atom     handles a number or ( ... )   (strongest)
```

For `2 + 3 * 4`:
1. `sum` asks `product` for the left side and gets `2`.
2. It sees `+`, so it asks `product` for the right side.
3. `product` reads `3`, sees `*`, reads `4`, and gives back `12`.
4. `sum` does `2 + 12 = 14`.

The `*` happened first because `product` sits deeper than `sum`. This pattern is called a recursive descent parser, and almost every calculator uses it.

### 11. `loop`

A `while (true)`. We leave it with `return`.

### 12. Percent rules

- In `atom`: `18% of 2450` becomes `0.18 * 2450`.
- In `sum`: `300 - 20%` means "minus 20% of 300", so we turn `20` into `300 * 20 / 100` before subtracting. This is how phone calculators behave.

### 13. `pub`

Only `calculate` and `format` have `pub`. Everything else is private to this file, like not exporting it in JS. The app can only call these two.

### 14. `format!`

```rust
let text = format!("{rounded}");
```

Like a JS template string: `` `${rounded}` ``.

### 15. Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn basic_math() {
        assert_eq!(calc("2 + 3 * 4"), Some("14".into()));
    }
}
```

- `#[cfg(test)]` means "only build this when testing".
- `#[test]` marks a test.
- `assert_eq!(a, b)` fails the test if `a` is not `b`.
- `"14".into()` turns the text into a `String` (owned text) so it can be compared.

Run them:

```bash
cargo test -p sidekick-calc
```

## Try it yourself

Small changes to practise, one at a time, each with a test:

1. **Bug hunt:** `"5 x 3"` gives `None`, even though the operator `match` has `'x'`. Find out why. (Hint: which `if` sees the letter x first?) Fix it and add a test.
2. **Easy:** add `"e"` as a word for `std::f64::consts::E`, next to `"pi"`.
3. **Medium:** support `sqrt(16)`. Add a `Sqrt` token for the word `"sqrt"`, then in `atom`, when you see `Sqrt`, read the next `atom` and use `.sqrt()` on it.
4. **Medium:** support thousand separators like `1,000`. Careful: `tokenize` reads `1` and `000` as two numbers if you just skip commas. Skip commas only inside the number loop.

When those pass, tell me and we do Step 2: wiring `calculate` into Ask as an instant result (one Tauri command and one small React row).
