//! himotoki-load — Rust port of `himotoki/loading/jmdict.py`.
//!
//! Streams JMdict XML into the same SQLite schema the runtime reads.
//! Entity references resolve to their entity *name* directly (equivalent to
//! Python's lxml-expand + `fix_entity_value` round-trip).

use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub mod conjugations;
pub mod errata;
mod errata_data;

// ============================================================================
// Schema (exact DDL from `himotoki.db.models`, kept byte-compatible)
// ============================================================================

pub const SCHEMA: &str = r#"
CREATE TABLE entry (
	seq INTEGER NOT NULL,
	content TEXT NOT NULL,
	root_p BOOLEAN NOT NULL,
	n_kanji INTEGER NOT NULL,
	n_kana INTEGER NOT NULL,
	primary_nokanji BOOLEAN NOT NULL,
	PRIMARY KEY (seq)
);
CREATE TABLE kanji_text (
	id INTEGER NOT NULL,
	seq INTEGER NOT NULL,
	text VARCHAR(255) NOT NULL,
	ord INTEGER NOT NULL,
	common INTEGER,
	common_tags VARCHAR(255) NOT NULL,
	conjugate_p BOOLEAN NOT NULL,
	nokanji BOOLEAN NOT NULL,
	best_kana VARCHAR(255),
	PRIMARY KEY (id),
	FOREIGN KEY(seq) REFERENCES entry (seq) ON DELETE CASCADE
);
CREATE TABLE kana_text (
	id INTEGER NOT NULL,
	seq INTEGER NOT NULL,
	text VARCHAR(255) NOT NULL,
	ord INTEGER NOT NULL,
	common INTEGER,
	common_tags VARCHAR(255) NOT NULL,
	conjugate_p BOOLEAN NOT NULL,
	nokanji BOOLEAN NOT NULL,
	best_kanji VARCHAR(255),
	PRIMARY KEY (id),
	FOREIGN KEY(seq) REFERENCES entry (seq) ON DELETE CASCADE
);
CREATE TABLE sense (
	id INTEGER NOT NULL,
	seq INTEGER NOT NULL,
	ord INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(seq) REFERENCES entry (seq) ON DELETE CASCADE
);
CREATE TABLE gloss (
	id INTEGER NOT NULL,
	sense_id INTEGER NOT NULL,
	text TEXT NOT NULL,
	ord INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(sense_id) REFERENCES sense (id) ON DELETE CASCADE
);
CREATE TABLE sense_prop (
	id INTEGER NOT NULL,
	sense_id INTEGER NOT NULL,
	seq INTEGER NOT NULL,
	tag VARCHAR(50) NOT NULL,
	text VARCHAR(255) NOT NULL,
	ord INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(sense_id) REFERENCES sense (id) ON DELETE CASCADE,
	FOREIGN KEY(seq) REFERENCES entry (seq) ON DELETE CASCADE
);
CREATE TABLE restricted_reading (
	id INTEGER NOT NULL,
	seq INTEGER NOT NULL,
	reading VARCHAR(255) NOT NULL,
	text VARCHAR(255) NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(seq) REFERENCES entry (seq) ON DELETE CASCADE
);
CREATE TABLE conjugation (
	id INTEGER NOT NULL,
	seq INTEGER NOT NULL,
	"from" INTEGER NOT NULL,
	via INTEGER,
	PRIMARY KEY (id),
	FOREIGN KEY(seq) REFERENCES entry (seq) ON DELETE CASCADE,
	FOREIGN KEY("from") REFERENCES entry (seq) ON DELETE CASCADE
);
CREATE TABLE conj_prop (
	id INTEGER NOT NULL,
	conj_id INTEGER NOT NULL,
	conj_type INTEGER NOT NULL,
	pos VARCHAR(50) NOT NULL,
	neg BOOLEAN,
	fml BOOLEAN,
	PRIMARY KEY (id),
	FOREIGN KEY(conj_id) REFERENCES conjugation (id) ON DELETE CASCADE
);
CREATE TABLE conj_source_reading (
	id INTEGER NOT NULL,
	conj_id INTEGER NOT NULL,
	text VARCHAR(255) NOT NULL,
	source_text VARCHAR(255) NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(conj_id) REFERENCES conjugation (id) ON DELETE CASCADE
);
CREATE INDEX ix_kanji_text_seq_ord ON kanji_text (seq, ord);
CREATE INDEX ix_kanji_text_text_cover ON kanji_text (text, seq, id, ord, common, best_kana);
CREATE INDEX ix_kana_text_seq_ord ON kana_text (seq, ord);
CREATE INDEX ix_kana_text_text_cover ON kana_text (text, seq, id, ord, common, best_kanji);
CREATE INDEX ix_sense_seq ON sense (seq);
CREATE INDEX ix_gloss_sense_id ON gloss (sense_id);
CREATE INDEX ix_sense_prop_seq_tag_text ON sense_prop (seq, tag, text);
CREATE INDEX ix_sense_prop_sense_id_tag ON sense_prop (sense_id, tag);
CREATE INDEX ix_sense_prop_tag_text ON sense_prop (tag, text);
CREATE INDEX ix_restricted_reading_seq_reading ON restricted_reading (seq, reading);
CREATE INDEX ix_conjugation_seq ON conjugation (seq);
CREATE INDEX ix_conjugation_from ON conjugation ("from");
CREATE INDEX ix_conjugation_from_via ON conjugation ("from", via);
CREATE INDEX ix_conj_prop_conj_id ON conj_prop (conj_id);
CREATE INDEX ix_conj_source_reading_conj_id ON conj_source_reading (conj_id);
"#;

/// Drop all tables (if present) and create a fresh schema.
pub fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys=OFF")?;
    for t in [
        "entry",
        "kanji_text",
        "kana_text",
        "sense",
        "gloss",
        "sense_prop",
        "restricted_reading",
        "conjugation",
        "conj_prop",
        "conj_source_reading",
    ] {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", t))?;
    }
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch("PRAGMA foreign_keys=ON")?;
    Ok(())
}

// ============================================================================
// Mini-DOM for one <entry>
// ============================================================================

#[derive(Debug, Default)]
struct Node {
    name: String,
    text: String,
    children: Vec<Node>,
}

impl Node {
    /// `node_text` — itertext equivalent: own text + all descendant text.
    fn node_text(&self) -> String {
        let mut s = self.text.clone();
        for c in &self.children {
            s.push_str(&c.node_text());
        }
        s
    }

    fn find(&self, tag: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == tag)
    }

    fn findall<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == tag)
    }
}

fn get_element_text(parent: &Node, tag: &str) -> Option<String> {
    parent.find(tag).map(|n| n.node_text())
}

fn get_elements_text(parent: &Node, tag: &str) -> Vec<String> {
    parent.findall(tag).map(|n| n.node_text()).collect()
}

/// Entity resolution matching Python's expand + `fix_entity_value` round-trip:
/// DTD entities (`&n;`, `&v1;`, `&ok;`, ...) are replaced by their entity NAME;
/// predefined XML entities and numeric refs resolve to their characters.
fn resolve_entities(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('&') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        match after.find(';') {
            Some(semi) => {
                let name = &after[..semi];
                let resolved: Option<String> = match name {
                    "lt" => Some("<".into()),
                    "gt" => Some(">".into()),
                    "amp" => Some("&".into()),
                    "apos" => Some("'".into()),
                    "quot" => Some("\"".into()),
                    _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string()),
                    _ if name.starts_with('#') => name[1..]
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string()),
                    // DTD entity → entity name (fix_entity_value equivalent).
                    // Only well-formed entity names — a stray "&text;" in a
                    // gloss stays literal.
                    _ if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '-' || c == '.') =>
                    {
                        Some(name.to_string())
                    }
                    _ => None,
                };
                match resolved {
                    Some(r) => {
                        out.push_str(&r);
                        rest = &after[semi + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = after;
                    }
                }
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

// ============================================================================
// Readings — `ParsedReading` / `parse_reading` / `insert_readings`
// ============================================================================

#[derive(Debug)]
struct ParsedReading {
    text: String,
    common: Option<i64>,
    nokanji: bool,
    pri_tags: String,
    restrictions: Vec<String>,
    skip: bool,
}

/// `parse_reading` — parse a `k_ele` (`keb`/`ke_pri`/`ke_restr`) or
/// `r_ele` (`reb`/`re_pri`/`re_inf`/`re_nokanji`/`re_restr`) element.
fn parse_reading(elem: &Node, text_tag: &str, pri_tag: &str) -> ParsedReading {
    let reading_text = get_element_text(elem, text_tag).unwrap_or_default();

    // outdated kana: re_inf == "ok" (entity name arrives directly)
    let skip = elem.findall("re_inf").any(|inf| inf.node_text() == "ok");

    let nokanji = elem.find("re_nokanji").is_some();

    let restr_tag = if text_tag == "reb" {
        "re_restr"
    } else {
        "ke_restr"
    };
    let restrictions = get_elements_text(elem, restr_tag);

    let mut common: Option<i64> = None;
    let mut pri_tags_list = Vec::new();
    for pri in elem.findall(pri_tag) {
        let pri_text = pri.node_text();
        pri_tags_list.push(pri_text.clone());
        if common.is_none() {
            common = Some(0);
        }
        if let Some(num) = pri_text.strip_prefix("nf") {
            if let Ok(v) = num.parse::<i64>() {
                common = Some(v);
            }
        }
    }
    let pri_tags = pri_tags_list
        .iter()
        .map(|t| format!("[{}]", t))
        .collect::<String>();

    ParsedReading {
        text: reading_text,
        common,
        nokanji,
        pri_tags,
        restrictions,
        skip,
    }
}

// ============================================================================
// Entry insertion — `load_entry` (statements cached on the connection)
// ============================================================================

/// `insert_readings` — returns (count, primary_nokanji).
fn insert_readings(
    tx: &Transaction,
    readings: &[ParsedReading],
    seq: i64,
    is_kana: bool,
) -> Result<(usize, bool)> {
    let mut primary_nokanji = false;
    let valid: Vec<&ParsedReading> = readings.iter().filter(|r| !r.skip).collect();
    let sql = if is_kana {
        "INSERT INTO kana_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kanji) \
         VALUES (?1,?2,?3,?4,?5,1,?6,NULL)"
    } else {
        "INSERT INTO kanji_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kana) \
         VALUES (?1,?2,?3,?4,?5,1,?6,NULL)"
    };

    for (ord_num, reading) in valid.iter().enumerate() {
        if is_kana && reading.nokanji {
            primary_nokanji = true;
        }
        let nokanji = if is_kana { reading.nokanji } else { false };
        tx.prepare_cached(sql)?.execute(params![
            seq,
            reading.text,
            ord_num as i64,
            reading.common,
            reading.pri_tags,
            nokanji,
        ])?;
        if is_kana {
            for restr in &reading.restrictions {
                tx.prepare_cached(
                    "INSERT INTO restricted_reading (seq,reading,text) VALUES (?1,?2,?3)",
                )?
                .execute(params![seq, reading.text, restr])?;
            }
        }
    }
    Ok((valid.len(), primary_nokanji))
}

const SENSE_TAGS: &[&str] = &["pos", "misc", "dial", "field", "s_inf", "stagk", "stagr"];

/// `insert_senses` — sense + gloss + sense_prop rows for one entry.
fn insert_senses(tx: &Transaction, sense_nodes: &[&Node], seq: i64) -> Result<()> {
    for (ord_num, sense_elem) in sense_nodes.iter().enumerate() {
        tx.prepare_cached("INSERT INTO sense (seq,ord) VALUES (?1,?2)")?
            .execute(params![seq, ord_num as i64])?;
        let sense_id = tx.last_insert_rowid();

        for (gloss_ord, gloss_elem) in sense_elem.findall("gloss").enumerate() {
            tx.prepare_cached("INSERT INTO gloss (sense_id,text,ord) VALUES (?1,?2,?3)")?
                .execute(params![sense_id, gloss_elem.node_text(), gloss_ord as i64])?;
        }
        for tag in SENSE_TAGS {
            for (prop_ord, elem) in sense_elem.findall(tag).enumerate() {
                // quick-xml yields entity names directly — same as
                // Python's expand + fix_entity_value round-trip.
                let text = elem.node_text();
                tx.prepare_cached(
                    "INSERT INTO sense_prop (sense_id,seq,tag,text,ord) VALUES (?1,?2,?3,?4,?5)",
                )?
                .execute(params![sense_id, seq, *tag, text, prop_ord as i64])?;
            }
        }
    }
    Ok(())
}

/// `load_entry` — returns Some(seq) or None if skipped.
fn load_entry(tx: &Transaction, entry: &Node, if_exists: &str) -> Result<Option<i64>> {
    let seq: i64 = match get_element_text(entry, "ent_seq") {
        Some(s) => s
            .trim()
            .parse()
            .with_context(|| format!("bad ent_seq: {s}"))?,
        None => return Ok(None),
    };

    let exists = tx
        .prepare_cached("SELECT 1 FROM entry WHERE seq = ?1 LIMIT 1")?
        .query_row([seq], |_| Ok(()))
        .optional()?
        .is_some();
    if exists {
        if if_exists == "skip" {
            return Ok(None);
        }
        tx.execute("DELETE FROM entry WHERE seq = ?1", [seq])?;
    }

    let kanji_readings: Vec<ParsedReading> = entry
        .findall("k_ele")
        .map(|e| parse_reading(e, "keb", "ke_pri"))
        .collect();
    let kana_readings: Vec<ParsedReading> = entry
        .findall("r_ele")
        .map(|e| parse_reading(e, "reb", "re_pri"))
        .collect();

    // Counts/flags are computable without inserting — Python's ORM flushes
    // `entry` before children, so insert it first (FK order).
    let n_kanji = kanji_readings.iter().filter(|r| !r.skip).count();
    let n_kana = kana_readings.iter().filter(|r| !r.skip).count();
    let primary_nokanji = kana_readings.iter().any(|r| !r.skip && r.nokanji);

    tx.prepare_cached(
        "INSERT INTO entry (seq,content,root_p,n_kanji,n_kana,primary_nokanji) \
         VALUES (?1,'',1,?2,?3,?4)",
    )?
    .execute(params![seq, n_kanji as i64, n_kana as i64, primary_nokanji])?;

    insert_readings(tx, &kanji_readings, seq, false)?;
    insert_readings(tx, &kana_readings, seq, true)?;

    let sense_nodes: Vec<&Node> = entry.findall("sense").collect();
    insert_senses(tx, &sense_nodes, seq)?;

    Ok(Some(seq))
}

// ============================================================================
// XML streaming — `iter_entries`
// ============================================================================

/// Stream each `<entry>` as a mini-DOM node.
fn iter_entries(
    reader: &mut Reader<BufReader<std::fs::File>>,
    mut f: impl FnMut(&Node) -> Result<()>,
) -> Result<()> {
    let mut buf = Vec::new();
    let mut stack: Vec<Node> = Vec::new();
    let mut in_entry = false;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "entry" {
                    in_entry = true;
                }
                if in_entry {
                    stack.push(Node {
                        name,
                        text: String::new(),
                        children: Vec::new(),
                    });
                }
            }
            Event::Text(e) => {
                if in_entry {
                    if let Some(top) = stack.last_mut() {
                        // JMdict is UTF-8; raw bytes decode directly.
                        // Entities are resolved by `resolve_entities`.
                        let raw = String::from_utf8_lossy(e.as_ref());
                        top.text.push_str(&resolve_entities(&raw));
                    }
                }
            }
            Event::CData(e) => {
                if in_entry {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&String::from_utf8_lossy(e.as_ref()));
                    }
                }
            }
            Event::End(e) => {
                if !in_entry {
                    continue;
                }
                if e.name() == QName(b"entry") {
                    if let Some(entry) = stack.pop() {
                        f(&entry)?;
                    }
                    stack.clear();
                    in_entry = false;
                } else if let Some(node) = stack.pop() {
                    if let Some(top) = stack.last_mut() {
                        top.children.push(node);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

// ============================================================================
// load_jmdict — main entry point
// ============================================================================

/// `load_jmdict` — stream JMdict XML into the DB. Returns entry count.
///
/// `if_exists`: "skip" (default) or "overwrite".
/// `batch_size`: commit every N entries (Python default 1000).
pub fn load_jmdict(
    conn: &mut Connection,
    xml_path: &Path,
    if_exists: &str,
    batch_size: usize,
    mut progress: impl FnMut(usize),
) -> Result<usize> {
    let file =
        std::fs::File::open(xml_path).with_context(|| format!("open {}", xml_path.display()))?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;

    let mut count = 0usize;

    // One transaction for the whole load — fastest correct option.
    // (Python commits every `batch_size` to keep the session small; rusqlite
    // has no session layer so a single tx is cheaper.)
    let tx = conn.transaction()?;
    iter_entries(&mut reader, |entry| {
        match load_entry(&tx, entry, if_exists) {
            Ok(Some(_)) => {
                count += 1;
                if count.is_multiple_of(batch_size) {
                    progress(count);
                }
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        }
    })?;
    tx.commit()?;
    progress(count);
    Ok(count)
}
