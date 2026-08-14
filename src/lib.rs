#![warn(clippy::all)]

//! Abaqus mesh input file (`.inp`) parser.
//!
//! Reads the mesh-defining keywords — `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`,
//! and `*INCLUDE` — and skips everything else (sections, materials, steps, ...).
//! Part/instance structure is flattened into one node/element namespace, and
//! sets with the same name are merged.

use std::borrow::Cow;
use std::iter::{Enumerate, Peekable};
use std::path::Path;
use std::str::Lines;
use std::{fmt, fs};

/// Node or element label. Abaqus caps labels at 999 999 999, so `u32` fits.
pub type Id = u32;

/// Parse error, with the 1-based line number where it occurred.
#[derive(Debug)]
pub struct Error {
    /// 1-based line number in the offending file (0 for I/O errors).
    pub line: usize,
    /// What went wrong.
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for Error {}

fn err(line: usize, msg: impl Into<String>) -> Error {
    Error {
        line: line + 1,
        msg: msg.into(),
    }
}

/// A named node or element set.
#[derive(Debug, PartialEq, Eq)]
pub struct Set {
    /// Set name as written in the file.
    pub name: String,
    /// Member labels, in file order; duplicates are kept.
    pub ids: Vec<Id>,
}

/// One `*ELEMENT` block: elements of a single type.
#[derive(Debug, PartialEq)]
pub struct ElementBlock {
    /// Abaqus element type name, e.g. `C3D8`.
    pub element_type: String,
    /// Element labels.
    pub ids: Vec<Id>,
    /// Flat connectivity: `nodes_per_element` node labels per element.
    pub connectivity: Vec<Id>,
    /// Nodes per element, inferred from the block's first element.
    pub nodes_per_element: usize,
}

impl ElementBlock {
    /// Iterate over `(element label, its node labels)`.
    pub fn elements(&self) -> impl Iterator<Item = (Id, &[Id])> {
        let stride = self.nodes_per_element.max(1);
        self.ids
            .iter()
            .copied()
            .zip(self.connectivity.chunks_exact(stride))
    }
}

/// Parsed mesh: flat node/element arrays plus named sets.
#[derive(Debug, Default, PartialEq)]
pub struct Mesh {
    /// Node labels, in file order.
    pub node_ids: Vec<Id>,
    /// Coordinates, parallel to `node_ids`; coordinates absent in the file are 0.
    pub node_coords: Vec<[f64; 3]>,
    /// One block per `*ELEMENT` keyword.
    pub element_blocks: Vec<ElementBlock>,
    /// `*NSET` sets, plus any `NSET=` parameter on `*NODE`.
    pub node_sets: Vec<Set>,
    /// `*ELSET` sets, plus any `ELSET=` parameter on `*ELEMENT`.
    pub element_sets: Vec<Set>,
}

impl Mesh {
    /// Look up a node set by name (Abaqus names are case-insensitive).
    pub fn node_set(&self, name: &str) -> Option<&Set> {
        self.node_sets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Look up an element set by name (case-insensitive).
    pub fn element_set(&self, name: &str) -> Option<&Set> {
        self.element_sets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Total element count across all blocks.
    pub fn element_count(&self) -> usize {
        self.element_blocks.iter().map(|b| b.ids.len()).sum()
    }
}

/// Parse from a string. `*INCLUDE` is an error here — use [`parse_file`].
pub fn parse_str(text: &str) -> Result<Mesh, Error> {
    let mut mesh = Mesh::default();
    parse_into(text, None, &mut mesh)?;
    Ok(mesh)
}

/// Parse a file, resolving `*INCLUDE` relative to the file's directory.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Mesh, Error> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|e| Error {
        line: 0,
        msg: format!("{}: {e}", path.display()),
    })?;
    let mut mesh = Mesh::default();
    parse_into(&text, path.parent(), &mut mesh)?;
    Ok(mesh)
}

type LineIter<'a> = Peekable<Enumerate<Lines<'a>>>;

/// What the data lines after the current keyword mean.
#[derive(Clone, Copy)]
enum Target {
    Skip,
    Node {
        nset: Option<usize>,
    },
    Element {
        block: usize,
        elset: Option<usize>,
    },
    Set {
        elem: bool,
        set: usize,
        generate: bool,
    },
}

fn parse_into(text: &str, dir: Option<&Path>, mesh: &mut Mesh) -> Result<(), Error> {
    let mut lines: LineIter<'_> = text.lines().enumerate().peekable();
    let mut target = Target::Skip;
    while let Some((n, raw)) = lines.next() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("**") {
            continue;
        }
        if let Some(kw_line) = line.strip_prefix('*') {
            let full = logical(kw_line, &mut lines);
            let (kw, params) = full.split_once(',').unwrap_or((&full, ""));
            target = keyword(mesh, dir, kw.trim(), params, n)?;
            continue;
        }
        if matches!(target, Target::Skip) {
            continue;
        }
        let full = logical(line, &mut lines);
        match target {
            Target::Skip => unreachable!(),
            Target::Node { nset } => {
                let mut fields = full.split(',').map(str::trim);
                let first = fields.next().unwrap_or("");
                if first.is_empty() {
                    continue;
                }
                let id = parse_id(first, n)?;
                let mut coords = [0.0; 3];
                for (c, f) in coords.iter_mut().zip(fields) {
                    if !f.is_empty() {
                        *c = f
                            .parse()
                            .map_err(|_| err(n, format!("bad coordinate `{f}`")))?;
                    }
                }
                mesh.node_ids.push(id);
                mesh.node_coords.push(coords);
                if let Some(s) = nset {
                    mesh.node_sets[s].ids.push(id);
                }
            }
            Target::Element { block, elset } => {
                let mut fields = full.split(',').map(str::trim).filter(|f| !f.is_empty());
                let Some(first) = fields.next() else {
                    continue;
                };
                let id = parse_id(first, n)?;
                let b = &mut mesh.element_blocks[block];
                let before = b.connectivity.len();
                for f in fields {
                    b.connectivity.push(parse_id(f, n)?);
                }
                let count = b.connectivity.len() - before;
                if b.nodes_per_element == 0 {
                    b.nodes_per_element = count;
                } else if count != b.nodes_per_element {
                    return Err(err(
                        n,
                        format!(
                            "element {id} has {count} nodes, block expects {}",
                            b.nodes_per_element
                        ),
                    ));
                }
                b.ids.push(id);
                if let Some(s) = elset {
                    mesh.element_sets[s].ids.push(id);
                }
            }
            Target::Set {
                elem,
                set,
                generate,
            } => {
                let sets = if elem {
                    &mut mesh.element_sets
                } else {
                    &mut mesh.node_sets
                };
                let mut fields = full.split(',').map(str::trim).filter(|f| !f.is_empty());
                if generate {
                    let Some(first) = fields.next() else {
                        continue;
                    };
                    let first = parse_id(first, n)?;
                    let last = fields
                        .next()
                        .ok_or_else(|| err(n, "GENERATE needs `first, last[, step]`"))?;
                    let last = parse_id(last, n)?;
                    let step = match fields.next() {
                        Some(f) => parse_id(f, n)?,
                        None => 1,
                    };
                    if step == 0 {
                        return Err(err(n, "GENERATE step must be nonzero"));
                    }
                    sets[set].ids.extend((first..=last).step_by(step as usize));
                } else {
                    for f in fields {
                        if let Ok(id) = f.parse::<Id>() {
                            sets[set].ids.push(id);
                        } else if let Some(src) =
                            sets.iter().position(|s| s.name.eq_ignore_ascii_case(f))
                        {
                            let ids = sets[src].ids.clone();
                            sets[set].ids.extend(ids);
                        } else {
                            return Err(err(n, format!("unknown set or bad label `{f}`")));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Handle a keyword line; returns what the following data lines mean.
fn keyword(
    mesh: &mut Mesh,
    dir: Option<&Path>,
    kw: &str,
    params: &str,
    n: usize,
) -> Result<Target, Error> {
    if kw.eq_ignore_ascii_case("NODE") {
        let nset = param(params, "NSET").map(|name| set_index(&mut mesh.node_sets, name));
        Ok(Target::Node { nset })
    } else if kw.eq_ignore_ascii_case("ELEMENT") {
        let ty = param(params, "TYPE").ok_or_else(|| err(n, "*ELEMENT without TYPE"))?;
        let elset = param(params, "ELSET").map(|name| set_index(&mut mesh.element_sets, name));
        mesh.element_blocks.push(ElementBlock {
            element_type: ty.to_owned(),
            ids: Vec::new(),
            connectivity: Vec::new(),
            nodes_per_element: 0,
        });
        Ok(Target::Element {
            block: mesh.element_blocks.len() - 1,
            elset,
        })
    } else if kw.eq_ignore_ascii_case("NSET") {
        let name = param(params, "NSET").ok_or_else(|| err(n, "*NSET without NSET name"))?;
        Ok(Target::Set {
            elem: false,
            set: set_index(&mut mesh.node_sets, name),
            generate: param(params, "GENERATE").is_some(),
        })
    } else if kw.eq_ignore_ascii_case("ELSET") {
        let name = param(params, "ELSET").ok_or_else(|| err(n, "*ELSET without ELSET name"))?;
        Ok(Target::Set {
            elem: true,
            set: set_index(&mut mesh.element_sets, name),
            generate: param(params, "GENERATE").is_some(),
        })
    } else if kw.eq_ignore_ascii_case("INCLUDE") {
        let input = param(params, "INPUT").ok_or_else(|| err(n, "*INCLUDE without INPUT"))?;
        let dir = dir.ok_or_else(|| err(n, "*INCLUDE needs parse_file (no base directory)"))?;
        let path = dir.join(input);
        let text =
            fs::read_to_string(&path).map_err(|e| err(n, format!("{}: {e}", path.display())))?;
        parse_into(&text, path.parent(), mesh).map_err(|e| Error {
            line: e.line,
            msg: format!("{}: {}", path.display(), e.msg),
        })?;
        Ok(Target::Skip)
    } else {
        Ok(Target::Skip)
    }
}

/// Join continuation lines: a line ending in `,` continues on the next data
/// line. A keyword line always terminates the join (real files leave trailing
/// commas on final data lines).
fn logical<'a>(first: &'a str, lines: &mut LineIter<'a>) -> Cow<'a, str> {
    if !first.ends_with(',') {
        return Cow::Borrowed(first);
    }
    let mut s = first.to_owned();
    while let Some(&(_, raw)) = lines.peek() {
        let t = raw.trim();
        if t.is_empty() || t.starts_with("**") {
            lines.next();
            continue;
        }
        if t.starts_with('*') {
            break;
        }
        lines.next();
        s.push_str(t);
        if !s.ends_with(',') {
            break;
        }
    }
    Cow::Owned(s)
}

/// Look up a keyword parameter, case-insensitively. Bare flags yield `""`.
fn param<'a>(params: &'a str, key: &str) -> Option<&'a str> {
    params.split(',').find_map(|p| {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        k.trim()
            .eq_ignore_ascii_case(key)
            .then(|| v.trim().trim_matches('"'))
    })
}

/// Index of the named set, creating it if new. Case-insensitive, so sets
/// repeated across instances merge.
fn set_index(sets: &mut Vec<Set>, name: &str) -> usize {
    sets.iter()
        .position(|s| s.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| {
            sets.push(Set {
                name: name.to_owned(),
                ids: Vec::new(),
            });
            sets.len() - 1
        })
}

fn parse_id(f: &str, n: usize) -> Result<Id, Error> {
    f.parse().map_err(|_| err(n, format!("bad label `{f}`")))
}

#[cfg(test)]
mod tests {
    use super::parse_str;

    #[test]
    fn continuation_and_generate() {
        let mesh = parse_str(
            "*NODE\n1, 0., 0.\n2, 1.,\n1.\n*ELEMENT, TYPE=B21\n1, 1,\n2\n\
             *NSET, NSET=n, GENERATE\n1, 2\n*ELSET, elset=e\n1,\n",
        )
        .unwrap();
        assert_eq!(mesh.node_ids, [1, 2]);
        assert_eq!(mesh.node_coords[1], [1.0, 1.0, 0.0]);
        let b = &mesh.element_blocks[0];
        assert_eq!(b.elements().collect::<Vec<_>>(), [(1, &[1, 2][..])]);
        assert_eq!(mesh.node_set("N").unwrap().ids, [1, 2]);
        assert_eq!(mesh.element_set("e").unwrap().ids, [1]);
    }

    #[test]
    fn skips_unknown_keywords() {
        let mesh = parse_str("*HEADING\nstuff, more\n*ELASTIC\n1., 2.\n*NODE\n1, 0.\n").unwrap();
        assert_eq!(mesh.node_ids, [1]);
    }

    #[test]
    fn error_carries_line_number() {
        assert_eq!(parse_str("*NODE\n1, 0.\nx, 0.\n").unwrap_err().line, 3);
    }

    #[test]
    fn include_needs_a_directory() {
        assert!(parse_str("*INCLUDE, INPUT=other.inp\n").is_err());
    }
}
