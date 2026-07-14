//! Catalogue hors ligne pour l'insertion rapide.
//!
//! Les émojis et symboles sont générés depuis les données officielles Unicode
//! et CLDR, puis embarqués dans le binaire avec `include_str!`. Aucune
//! connexion réseau n'est nécessaire pendant l'utilisation de ClipH.

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Grande section du panneau d'insertion rapide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuickInsertCategory {
    Emoji,
    Kaomoji,
    Symbol,
}

impl QuickInsertCategory {
    pub const fn display_label(self) -> &'static str {
        match self {
            Self::Emoji => "Émojis",
            Self::Kaomoji => "Kaomojis",
            Self::Symbol => "Symboles",
        }
    }

    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Emoji => "face-smile-symbolic",
            Self::Kaomoji => "avatar-default-symbolic",
            Self::Symbol => "accessories-character-map-symbolic",
        }
    }
}

/// Élément insérable dans le presse-papiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickInsertEntry {
    pub category: QuickInsertCategory,
    pub group: &'static str,
    pub value: &'static str,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
    normalized_value: &'static str,
    normalized_label: &'static str,
    normalized_group: &'static str,
    normalized_keywords: &'static str,
}

const EMOJI_DATA: &str = include_str!("../data/emoji.tsv");
const SYMBOL_DATA: &str = include_str!("../data/symbols.tsv");

const KAOMOJI_DATA: &str = r#"
Joie	(＾▽＾)	Grand sourire	heureux|joie
Joie	(◕‿◕)	Sourire doux	heureux|mignon
Joie	ヽ(・∀・)ﾉ	Célébration	fête|joie
Joie	٩(◕‿◕｡)۶	Très heureux	joie|victoire
Amour	(づ｡◕‿‿◕｡)づ	Câlin	amour|tendresse
Amour	(♡°▽°♡)	Amoureux	cœur|amour
Amour	(っ˘з(˘⌣˘ )	Bisou	baiser|amour
Tristesse	(╥﹏╥)	Pleure	triste|larmes
Tristesse	(｡•́︿•̀｡)	Déçu	triste|déception
Tristesse	ಥ_ಥ	Larmes	pleurer|triste
Colère	(╬ಠ益ಠ)	Très en colère	rage|fâché
Colère	ヽ( `д´*)ノ	Colère	fâché|crier
Surprise	(⊙_⊙)	Surpris	étonné|choc
Surprise	Σ(°ロ°)	Choqué	surprise|peur
Actions	¯\_(ツ)_/¯	Haussement d'épaules	je ne sais pas|bof
Actions	(ง'̀-'́)ง	Prêt à combattre	force|courage
Actions	(－‸ლ)	Facepalm	déçu|erreur
Actions	(╯°□°）╯︵ ┻━┻	Renverse la table	colère|table
Actions	┬─┬ノ( º _ ºノ)	Remet la table	calme|table
Animaux	ʕ•ᴥ•ʔ	Ours	animal|mignon
Animaux	(=^･ω･^=)	Chat	animal|mignon
Animaux	／(≧ x ≦)＼	Lapin	animal|mignon
"#;

/// Catalogue complet chargé une seule fois.
pub fn catalog() -> &'static [QuickInsertEntry] {
    static CATALOG: OnceLock<Vec<QuickInsertEntry>> = OnceLock::new();

    CATALOG
        .get_or_init(|| {
            let mut entries = Vec::new();

            parse_tsv(QuickInsertCategory::Emoji, EMOJI_DATA, &mut entries);
            parse_tsv(QuickInsertCategory::Kaomoji, KAOMOJI_DATA, &mut entries);
            parse_tsv(QuickInsertCategory::Symbol, SYMBOL_DATA, &mut entries);

            entries
        })
        .as_slice()
}

/// Retourne tous les éléments d'une catégorie.
pub fn entries_for_category(category: QuickInsertCategory) -> Vec<&'static QuickInsertEntry> {
    catalog()
        .iter()
        .filter(|entry| entry.category == category)
        .collect()
}

/// Retourne la liste ordonnée des groupes d'une catégorie.
pub fn groups_for_category(category: QuickInsertCategory) -> Vec<&'static str> {
    catalog()
        .iter()
        .filter(|entry| entry.category == category)
        .map(|entry| entry.group)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Recherche dans une catégorie.
///
/// La recherche ignore la casse et les principaux accents français. Tous les
/// mots saisis doivent être présents dans la valeur, le nom, le groupe ou les
/// mots-clés de l'entrée.
pub fn search_entries(
    category: QuickInsertCategory,
    query: &str,
    limit: usize,
) -> Vec<&'static QuickInsertEntry> {
    if limit == 0 {
        return Vec::new();
    }

    let normalized_query = normalize_for_search(query);
    let terms = normalized_query.split_whitespace().collect::<Vec<_>>();

    let mut matches = catalog()
        .iter()
        .filter(|entry| entry.category == category)
        .filter_map(|entry| search_score(entry, &terms).map(|score| (score, entry)))
        .collect::<Vec<_>>();

    matches.sort_by_key(|(score, entry)| {
        (
            Reverse(*score),
            entry.group,
            entry.normalized_label,
            entry.value,
        )
    });

    matches
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}

pub fn find_entry_by_value(
    category: QuickInsertCategory,
    value: &str,
) -> Option<&'static QuickInsertEntry> {
    catalog()
        .iter()
        .find(|entry| entry.category == category && entry.value == value)
}

fn parse_tsv(
    category: QuickInsertCategory,
    data: &'static str,
    entries: &mut Vec<QuickInsertEntry>,
) {
    for line in data
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let mut columns = line.splitn(4, '\t');

        let Some(group) = columns.next() else {
            continue;
        };
        let Some(value) = columns.next() else {
            continue;
        };
        let Some(label) = columns.next() else {
            continue;
        };
        let keywords_text = columns.next().unwrap_or_default();

        if group.is_empty() || value.is_empty() || label.is_empty() {
            continue;
        }

        let group = leak_string(group.to_owned());
        let value = leak_string(value.to_owned());
        let label = leak_string(label.to_owned());

        let keywords = keywords_text
            .split('|')
            .map(str::trim)
            .filter(|keyword| !keyword.is_empty())
            .map(|keyword| leak_string(keyword.to_owned()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let keywords: &'static [&'static str] = Box::leak(keywords.into_boxed_slice());

        let normalized_value = leak_string(normalize_for_search(value));
        let normalized_label = leak_string(normalize_for_search(label));
        let normalized_group = leak_string(normalize_for_search(group));
        let normalized_keywords = leak_string(normalize_for_search(&keywords.join(" ")));

        entries.push(QuickInsertEntry {
            category,
            group,
            value,
            label,
            keywords,
            normalized_value,
            normalized_label,
            normalized_group,
            normalized_keywords,
        });
    }
}

fn leak_string(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn search_score(entry: &QuickInsertEntry, terms: &[&str]) -> Option<u32> {
    if terms.is_empty() {
        return Some(1);
    }

    let mut score = 0_u32;

    for term in terms {
        let term_score = if entry.normalized_value == *term {
            120
        } else if entry.normalized_label == *term {
            100
        } else if entry.normalized_label.starts_with(term) {
            80
        } else if entry.normalized_label.contains(term) {
            65
        } else if entry.normalized_group.starts_with(term) {
            50
        } else if entry.normalized_group.contains(term) {
            40
        } else if entry.normalized_keywords.contains(term) {
            35
        } else {
            return None;
        };

        score += term_score;
    }

    Some(score)
}

fn normalize_for_search(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_space = false;

    for character in value.chars().flat_map(char::to_lowercase) {
        let replacement = match character {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            'œ' => 'o',
            character => character,
        };

        let is_separator = replacement.is_whitespace()
            || matches!(
                replacement,
                '-' | '_' | ',' | ';' | ':' | '/' | '\\' | '(' | ')' | '[' | ']' | '|' | '’' | '\''
            );

        if is_separator {
            if !previous_was_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            previous_was_space = true;
        } else {
            normalized.push(replacement);
            previous_was_space = false;
        }
    }

    normalized.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_all_offline_categories() {
        assert!(entries_for_category(QuickInsertCategory::Emoji).len() >= 3_500);
        assert!(entries_for_category(QuickInsertCategory::Kaomoji).len() >= 20);
        assert!(entries_for_category(QuickInsertCategory::Symbol).len() >= 5_000);
    }

    #[test]
    fn values_are_unique_inside_each_category() {
        for category in [
            QuickInsertCategory::Emoji,
            QuickInsertCategory::Kaomoji,
            QuickInsertCategory::Symbol,
        ] {
            let entries = entries_for_category(category);
            let unique_values = entries
                .iter()
                .map(|entry| entry.value)
                .collect::<BTreeSet<_>>();

            assert_eq!(unique_values.len(), entries.len());
        }
    }

    #[test]
    fn search_is_case_and_accent_insensitive() {
        let results = search_entries(QuickInsertCategory::Emoji, "CELEBRATION", 30);

        assert!(results.iter().any(|entry| entry.value == "🥳"));
    }

    #[test]
    fn search_finds_french_keywords() {
        let results = search_entries(QuickInsertCategory::Emoji, "piece jointe", 30);

        assert!(results.iter().any(|entry| entry.value == "📎"));
    }

    #[test]
    fn search_finds_kaomoji_actions() {
        let results = search_entries(QuickInsertCategory::Kaomoji, "je ne sais pas", 30);

        assert!(results.iter().any(|entry| entry.value == "¯\\_(ツ)_/¯"));
    }

    #[test]
    fn search_finds_mathematical_symbols() {
        let results = search_entries(QuickInsertCategory::Symbol, "inferieur egal", 30);

        assert!(results.iter().any(|entry| entry.value == "≤"));
    }

    #[test]
    fn empty_query_returns_limited_category_entries() {
        let results = search_entries(QuickInsertCategory::Symbol, "", 7);

        assert_eq!(results.len(), 7);
        assert!(
            results
                .iter()
                .all(|entry| { entry.category == QuickInsertCategory::Symbol })
        );
    }

    #[test]
    fn zero_limit_returns_no_entry() {
        assert!(search_entries(QuickInsertCategory::Emoji, "sourire", 0,).is_empty());
    }

    #[test]
    fn groups_are_unique_and_ordered() {
        let groups = groups_for_category(QuickInsertCategory::Symbol);

        assert!(groups.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(groups.contains(&"Mathématiques"));
        assert!(groups.contains(&"Monnaies"));
    }

    #[test]
    fn value_lookup_respects_category() {
        assert!(find_entry_by_value(QuickInsertCategory::Emoji, "😀",).is_some());

        assert!(find_entry_by_value(QuickInsertCategory::Symbol, "😀",).is_none());
    }
}
