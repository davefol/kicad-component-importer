use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    value: String,
    quoted: bool,
}

impl Atom {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            quoted: false,
        }
    }

    pub fn new_quoted(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            quoted: true,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sexp {
    Atom(Atom),
    List(Vec<Sexp>),
}

impl Sexp {
    pub fn to_string_pretty(&self) -> String {
        self.to_string_pretty_with_indent("\t")
    }

    pub fn to_string_pretty_with_indent(&self, indent_str: &str) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0, indent_str);
        out.push('\n');
        out
    }

    fn write_pretty(&self, out: &mut String, indent: usize, indent_str: &str) {
        match self {
            Sexp::Atom(atom) => out.push_str(&render_atom(atom)),
            Sexp::List(items) => {
                out.push('(');
                if items.is_empty() {
                    out.push(')');
                    return;
                }
                if items.iter().all(|item| matches!(item, Sexp::Atom(_))) {
                    for (idx, item) in items.iter().enumerate() {
                        if idx > 0 {
                            out.push(' ');
                        }
                        item.write_pretty(out, indent, indent_str);
                    }
                    out.push(')');
                    return;
                }
                items[0].write_pretty(out, indent, indent_str);
                for item in &items[1..] {
                    out.push('\n');
                    for _ in 0..indent + 1 {
                        out.push_str(indent_str);
                    }
                    item.write_pretty(out, indent + 1, indent_str);
                }
                out.push('\n');
                for _ in 0..indent {
                    out.push_str(indent_str);
                }
                out.push(')');
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Symbol {
    name: String,
    sexp: Sexp,
}

impl Symbol {
    pub fn parse(input: &str) -> Result<Self, KicadSymError> {
        let mut parser = Parser::new(input);
        let mut items = parser.parse_all()?;
        if items.len() != 1 {
            return Err(KicadSymError::new(
                "expected a single top-level S-expression for symbol",
            ));
        }
        Symbol::from_sexp(items.remove(0))
    }

    pub fn from_sexp(sexp: Sexp) -> Result<Self, KicadSymError> {
        let name = symbol_name(&sexp).ok_or_else(|| {
            KicadSymError::new("symbol must be a list like (symbol <name> ...)")
        })?;
        Ok(Self {
            name: name.to_string(),
            sexp,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn property_value(&self, name: &str) -> Option<String> {
        let list = match &self.sexp {
            Sexp::List(items) => items,
            _ => return None,
        };
        for item in list {
            if let Some(value) = property_value(item, name) {
                return Some(value.to_string());
            }
        }
        None
    }

    pub fn set_property_value(&mut self, name: &str, value: &str) -> bool {
        let list = match &mut self.sexp {
            Sexp::List(items) => items,
            _ => return false,
        };
        for item in list.iter_mut() {
            if let Some(items) = property_items_mut(item, name) {
                if items.len() >= 3 {
                    items[2] = Sexp::Atom(Atom::new(value));
                    return true;
                }
            }
        }
        false
    }

    pub fn set_or_add_property(&mut self, name: &str, value: &str) {
        if self.set_property_value(name, value) {
            return;
        }
        let list = match &mut self.sexp {
            Sexp::List(items) => items,
            _ => return,
        };
        if let Some(template) = list.iter().find_map(|item| match item {
            Sexp::List(items) if is_property_list(items) => Some(items.clone()),
            _ => None,
        }) {
            let mut new_items = template;
            if new_items.len() >= 2 {
                new_items[1] = Sexp::Atom(Atom::new_quoted(name));
            } else {
                new_items.push(Sexp::Atom(Atom::new_quoted(name)));
            }
            if new_items.len() >= 3 {
                new_items[2] = Sexp::Atom(Atom::new(value));
            } else {
                new_items.push(Sexp::Atom(Atom::new(value)));
            }
            list.push(Sexp::List(new_items));
            return;
        }
        list.push(Sexp::List(vec![
            Sexp::Atom(Atom::new("property")),
            Sexp::Atom(Atom::new_quoted(name)),
            Sexp::Atom(Atom::new(value)),
        ]));
    }

    pub fn into_sexp(self) -> Sexp {
        self.sexp
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddPolicy {
    ErrorOnConflict,
    ReplaceExisting,
    SkipExisting,
}

#[derive(Clone, Debug)]
pub struct KicadSymbolLib {
    root: Sexp,
}

impl KicadSymbolLib {
    pub fn parse(input: &str) -> Result<Self, KicadSymError> {
        let mut parser = Parser::new(input);
        let mut items = parser.parse_all()?;
        if items.len() != 1 {
            return Err(KicadSymError::new(
                "expected a single top-level S-expression for library",
            ));
        }
        let root = items.remove(0);
        ensure_root(&root)?;
        Ok(Self { root })
    }

    pub fn symbols(&self) -> Result<Vec<Symbol>, KicadSymError> {
        let items = root_items(&self.root)?;
        let mut out = Vec::new();
        for item in items.iter().skip(1) {
            if symbol_name(item).is_some() {
                out.push(Symbol::from_sexp(item.clone())?);
            }
        }
        Ok(out)
    }

    pub fn add_symbol(
        &mut self,
        symbol: Symbol,
        policy: AddPolicy,
    ) -> Result<(), KicadSymError> {
        ensure_root(&self.root)?;
        let items = root_items_mut(&mut self.root)?;
        let name = symbol.name().to_string();
        let mut existing = None;
        for (idx, item) in items.iter().enumerate().skip(1) {
            if symbol_name(item) == Some(name.as_str()) {
                existing = Some(idx);
                break;
            }
        }
        match (existing, policy) {
            (Some(_), AddPolicy::SkipExisting) => Ok(()),
            (Some(_), AddPolicy::ErrorOnConflict) => Err(KicadSymError::new(format!(
                "symbol already exists: {}",
                name
            ))),
            (Some(idx), AddPolicy::ReplaceExisting) => {
                items[idx] = symbol.into_sexp();
                Ok(())
            }
            (None, _) => {
                items.push(symbol.into_sexp());
                Ok(())
            }
        }
    }

    pub fn to_string_pretty(&self) -> String {
        self.root.to_string_pretty_with_indent("\t")
    }
}

pub fn parse_sexps(input: &str) -> Result<Vec<Sexp>, KicadSymError> {
    let mut parser = Parser::new(input);
    parser.parse_all()
}

pub fn parse_one(input: &str) -> Result<Sexp, KicadSymError> {
    let mut items = parse_sexps(input)?;
    if items.len() != 1 {
        return Err(KicadSymError::new(
            "expected a single top-level S-expression",
        ));
    }
    Ok(items.remove(0))
}

#[derive(Debug, Clone)]
pub struct KicadSymError {
    message: String,
    line: Option<usize>,
    column: Option<usize>,
}

impl KicadSymError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            column: None,
        }
    }

    fn with_pos(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line: Some(line),
            column: Some(column),
        }
    }
}

impl fmt::Display for KicadSymError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(f, "{} at {}:{}", self.message, line, column)
            }
            _ => write!(f, "{}", self.message),
        }
    }
}

impl Error for KicadSymError {}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn parse_all(&mut self) -> Result<Vec<Sexp>, KicadSymError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.peek().is_none() {
                break;
            }
            items.push(self.parse_sexp()?);
        }
        Ok(items)
    }

    fn parse_sexp(&mut self) -> Result<Sexp, KicadSymError> {
        self.skip_ws_and_comments();
        match self.peek() {
            Some('(') => self.parse_list(),
            Some('"') => self.parse_quoted_atom(),
            Some(')') => Err(self.error("unexpected ')'")),
            Some(_) => self.parse_bare_atom(),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn parse_list(&mut self) -> Result<Sexp, KicadSymError> {
        self.expect('(')?;
        let mut items = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                Some(')') => {
                    self.next();
                    break;
                }
                None => return Err(self.error("unterminated list")),
                _ => items.push(self.parse_sexp()?),
            }
        }
        Ok(Sexp::List(items))
    }

    fn parse_bare_atom(&mut self) -> Result<Sexp, KicadSymError> {
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() || matches!(ch, '(' | ')' | '"' | ';' | '#') {
                break;
            }
            self.next();
            value.push(ch);
        }
        if value.is_empty() {
            return Err(self.error("expected atom"));
        }
        Ok(Sexp::Atom(Atom::new(value)))
    }

    fn parse_quoted_atom(&mut self) -> Result<Sexp, KicadSymError> {
        self.expect('"')?;
        let mut value = String::new();
        loop {
            let ch = self.next().ok_or_else(|| self.error("unterminated string"))?;
            match ch {
                '"' => break,
                '\\' => {
                    let esc = self.next().ok_or_else(|| self.error("unterminated escape"))?;
                    match esc {
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        _ => value.push(esc),
                    }
                }
                _ => value.push(ch),
            }
        }
        Ok(Sexp::Atom(Atom::new_quoted(value)))
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(';') | Some('#') => self.consume_comment(),
                _ => break,
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if !ch.is_whitespace() {
                break;
            }
            self.next();
        }
    }

    fn consume_comment(&mut self) {
        while let Some(ch) = self.next() {
            if ch == '\n' {
                break;
            }
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), KicadSymError> {
        match self.next() {
            Some(ch) if ch == expected => Ok(()),
            _ => Err(self.error(format!("expected '{}'", expected))),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn error(&self, message: impl Into<String>) -> KicadSymError {
        KicadSymError::with_pos(message, self.line, self.column)
    }
}

fn root_items(sexp: &Sexp) -> Result<&Vec<Sexp>, KicadSymError> {
    match sexp {
        Sexp::List(items) => Ok(items),
        _ => Err(KicadSymError::new(
            "library root must be a list expression",
        )),
    }
}

fn root_items_mut(sexp: &mut Sexp) -> Result<&mut Vec<Sexp>, KicadSymError> {
    match sexp {
        Sexp::List(items) => Ok(items),
        _ => Err(KicadSymError::new(
            "library root must be a list expression",
        )),
    }
}

fn ensure_root(sexp: &Sexp) -> Result<(), KicadSymError> {
    let items = root_items(sexp)?;
    if items.is_empty() {
        return Err(KicadSymError::new("library root list is empty"));
    }
    match atom_value(&items[0]) {
        Some("kicad_symbol_lib") => Ok(()),
        _ => Err(KicadSymError::new(
            "expected root list to start with kicad_symbol_lib",
        )),
    }
}

fn symbol_name(sexp: &Sexp) -> Option<&str> {
    let items = match sexp {
        Sexp::List(items) => items,
        _ => return None,
    };
    if items.len() < 2 {
        return None;
    }
    if atom_value(&items[0]) != Some("symbol") {
        return None;
    }
    atom_value(&items[1])
}

fn atom_value(sexp: &Sexp) -> Option<&str> {
    match sexp {
        Sexp::Atom(atom) => Some(atom.value()),
        _ => None,
    }
}

fn property_value<'a>(sexp: &'a Sexp, name: &str) -> Option<&'a str> {
    let items = match sexp {
        Sexp::List(items) => items,
        _ => return None,
    };
    if items.len() < 3 {
        return None;
    }
    if atom_value(&items[0]) != Some("property") {
        return None;
    }
    if atom_value(&items[1]) != Some(name) {
        return None;
    }
    atom_value(&items[2])
}

fn property_items_mut<'a>(sexp: &'a mut Sexp, name: &str) -> Option<&'a mut Vec<Sexp>> {
    let items = match sexp {
        Sexp::List(items) => items,
        _ => return None,
    };
    if items.len() < 3 {
        return None;
    }
    if !is_property_list(items) {
        return None;
    }
    if atom_value(&items[1]) != Some(name) {
        return None;
    }
    Some(items)
}

fn is_property_list(items: &[Sexp]) -> bool {
    if items.is_empty() {
        return false;
    }
    atom_value(&items[0]) == Some("property")
}

fn render_atom(atom: &Atom) -> String {
    if atom.quoted || needs_quotes(atom.value()) {
        format!("\"{}\"", escape_atom(atom.value()))
    } else {
        atom.value().to_string()
    }
}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value.chars().any(|ch| {
            ch.is_whitespace() || matches!(ch, '(' | ')' | '"' | ';' | '#')
        })
}

fn escape_atom(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_symbols_from_library() {
        let input = "(kicad_symbol_lib (version 20231120) (symbol \"A\") (symbol \"B\"))";
        let lib = KicadSymbolLib::parse(input).unwrap();
        let names: Vec<_> = lib
            .symbols()
            .unwrap()
            .into_iter()
            .map(|sym| sym.name().to_string())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn add_symbol_replaces_existing() {
        let input = "(kicad_symbol_lib (version 20231120) (symbol \"A\"))";
        let mut lib = KicadSymbolLib::parse(input).unwrap();
        let symbol = Symbol::parse("(symbol \"A\" (property \"Value\" \"new\"))").unwrap();
        lib.add_symbol(symbol, AddPolicy::ReplaceExisting)
            .unwrap();
        let out = lib.to_string_pretty();
        assert!(out.contains("new"));
    }

    #[test]
    fn add_symbol_skips_existing() {
        let input = "(kicad_symbol_lib (version 20231120) (symbol \"A\"))";
        let mut lib = KicadSymbolLib::parse(input).unwrap();
        let symbol = Symbol::parse("(symbol \"A\" (property \"Value\" \"new\"))").unwrap();
        lib.add_symbol(symbol, AddPolicy::SkipExisting).unwrap();
        let out = lib.to_string_pretty();
        assert!(!out.contains("new"));
    }

    #[test]
    fn add_symbol_errors_on_conflict() {
        let input = "(kicad_symbol_lib (version 20231120) (symbol \"A\"))";
        let mut lib = KicadSymbolLib::parse(input).unwrap();
        let symbol = Symbol::parse("(symbol \"A\")").unwrap();
        let err = lib
            .add_symbol(symbol, AddPolicy::ErrorOnConflict)
            .unwrap_err();
        assert!(err.to_string().contains("symbol already exists"));
    }

    #[test]
    fn roundtrip_preserves_symbol_names() {
        let input = "(kicad_symbol_lib (version 20231120) (symbol \"A\") (symbol \"B\"))";
        let lib = KicadSymbolLib::parse(input).unwrap();
        let out = lib.to_string_pretty();
        let lib_again = KicadSymbolLib::parse(&out).unwrap();
        let names: Vec<_> = lib_again
            .symbols()
            .unwrap()
            .into_iter()
            .map(|sym| sym.name().to_string())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn parses_comments_and_quoted_names() {
        let input = "(kicad_symbol_lib\n; comment\n(symbol \"LM 2907-8\")\n# comment\n)";
        let lib = KicadSymbolLib::parse(input).unwrap();
        let names: Vec<_> = lib
            .symbols()
            .unwrap()
            .into_iter()
            .map(|sym| sym.name().to_string())
            .collect();
        assert_eq!(names, vec!["LM 2907-8"]);
        let out = lib.to_string_pretty();
        assert!(out.contains("\"LM 2907-8\""));
    }

    #[test]
    fn set_property_updates_existing_value() {
        let mut symbol = Symbol::parse("(symbol \"A\" (property \"Footprint\" \"\"))").unwrap();
        assert_eq!(symbol.property_value("Footprint").unwrap(), "");
        symbol.set_property_value("Footprint", "Lib:FP");
        assert_eq!(symbol.property_value("Footprint").unwrap(), "Lib:FP");
    }

    #[test]
    fn set_or_add_property_inserts_when_missing() {
        let mut symbol = Symbol::parse("(symbol \"A\")").unwrap();
        assert!(symbol.property_value("Footprint").is_none());
        symbol.set_or_add_property("Footprint", "Lib:FP");
        assert_eq!(symbol.property_value("Footprint").unwrap(), "Lib:FP");
    }
}
