pub const ADR_MAX_LENGTH: usize = 8_000;
pub const ADR_MAX_SECTIONS: usize = 16;

const CANONICAL_SECTIONS: [&str; 6] = [
    "PURPOSE",
    "STACK",
    "ARCHITECTURE",
    "PATTERNS",
    "TRADEOFFS",
    "PHILOSOPHY",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdrSection {
    pub name: String,
    pub content: String,
}

impl AdrSection {
    #[must_use]
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
        }
    }
}

#[must_use]
pub fn parse_adr_sections(content: &str) -> Vec<AdrSection> {
    let mut sections = Vec::new();
    let mut current_name: Option<&str> = None;
    let mut current_lines = Vec::new();

    for line in content.split('\n') {
        if let Some(name) = canonical_header(line) {
            save_section(&mut sections, current_name.take(), &current_lines);
            current_name = Some(name);
            current_lines.clear();
        } else if current_name.is_some() && (!current_lines.is_empty() || !line.is_empty()) {
            current_lines.push(line);
        }
    }
    save_section(&mut sections, current_name, &current_lines);
    sections
}

#[must_use]
pub fn render_adr_sections(sections: &[AdrSection]) -> String {
    if sections.is_empty() {
        return String::new();
    }

    let mut rendered = vec![false; sections.len()];
    let mut ordered = Vec::with_capacity(sections.len());
    for canonical in CANONICAL_SECTIONS {
        if let Some((index, section)) = sections
            .iter()
            .enumerate()
            .find(|(index, section)| !rendered[*index] && section.name == canonical)
        {
            rendered[index] = true;
            ordered.push(section);
        }
    }

    let mut extra = sections
        .iter()
        .enumerate()
        .filter(|(index, _)| !rendered[*index])
        .collect::<Vec<_>>();
    extra.sort_by(|(_, left), (_, right)| left.name.cmp(&right.name));
    ordered.extend(extra.into_iter().map(|(_, section)| section));

    ordered
        .into_iter()
        .map(|section| format!("## {}\n{}", section.name, section.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn canonical_header(line: &str) -> Option<&str> {
    let header = line
        .strip_prefix("## ")?
        .trim_end_matches([' ', '\t', '\r']);
    CANONICAL_SECTIONS.contains(&header).then_some(header)
}

fn save_section(sections: &mut Vec<AdrSection>, name: Option<&str>, lines: &[&str]) {
    if sections.len() >= ADR_MAX_SECTIONS {
        return;
    }
    let Some(name) = name else {
        return;
    };
    let content = lines.join("\n");
    let content = content.trim_matches(['\n', ' ']);
    sections.push(AdrSection::new(name, content));
}
