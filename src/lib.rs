#![warn(clippy::all)]

//! Abaqus mesh input file (`.inp`) parser.
//!
//! Reads the mesh-defining keywords — `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`,
//! `*PART`, `*INSTANCE`, `*MATERIAL`, `*DENSITY`, and `*INCLUDE` — and skips
//! everything else (sections, steps, physics, ...). This covers the subset MCNP
//! requires of Abaqus-formatted unstructured mesh files (MCNP 6.3.1 §8.7).
//!
//! Node and element numbering is local to each [`Part`]. Mesh defined outside
//! any part (flat files), or at assembly level, lands in an implicit part named
//! `""`. Mesh defined inside an `*INSTANCE` block is attached to the instance's
//! part, as are assembly-level sets carrying an `INSTANCE=` parameter.

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

/// A `*PART` block: a mesh with its own node/element numbering.
#[derive(Debug, Default, PartialEq)]
pub struct Part {
    /// Part name; `""` for mesh defined outside any part.
    pub name: String,
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

impl Part {
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

/// Rotation of an instance: an axis through two points and an angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    /// First point on the rotation axis.
    pub axis_a: [f64; 3],
    /// Second point on the rotation axis.
    pub axis_b: [f64; 3],
    /// Rotation angle in degrees.
    pub angle_deg: f64,
}

/// An `*INSTANCE` of a part within the assembly. The translation is applied
/// before the rotation (MCNP 6.3.1 §8.7.2.7).
#[derive(Debug, PartialEq)]
pub struct Instance {
    /// Instance name.
    pub name: String,
    /// Name of the instanced [`Part`].
    pub part: String,
    /// Translation in x, y, z; zero if absent.
    pub translation: [f64; 3],
    /// Optional rotation, applied after the translation.
    pub rotation: Option<Rotation>,
}

/// A `*MATERIAL` definition.
#[derive(Debug, PartialEq)]
pub struct Material {
    /// Material name. MCNP matches its trailing number against material elsets.
    pub name: String,
    /// First `*DENSITY` value, if given.
    pub density: Option<f64>,
}

/// Parsed model: parts, the instances assembling them, and materials.
#[derive(Debug, Default, PartialEq)]
pub struct Mesh {
    /// Parts, in file order. Flat files yield one part named `""`.
    pub parts: Vec<Part>,
    /// Assembly instances, in file order.
    pub instances: Vec<Instance>,
    /// Materials, in file order.
    pub materials: Vec<Material>,
}

impl Mesh {
    /// Look up a part by name (case-insensitive).
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Total element count across all parts (not weighted by instancing).
    pub fn element_count(&self) -> usize {
        self.parts.iter().map(Part::element_count).sum()
    }
}

/// Parse from a string. `*INCLUDE` is an error here — use [`parse_file`].
pub fn parse_str(text: &str) -> Result<Mesh, Error> {
    let mut mesh = Mesh::default();
    parse_into(text, None, &mut mesh, None)?;
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
    parse_into(&text, path.parent(), &mut mesh, None)?;
    Ok(mesh)
}

type LineIter<'a> = Peekable<Enumerate<Lines<'a>>>;

/// What the data lines after the current keyword mean.
#[derive(Clone, Copy)]
enum Target {
    Skip,
    Node {
        part: usize,
        nset: Option<usize>,
    },
    Element {
        part: usize,
        block: usize,
        elset: Option<usize>,
    },
    Set {
        part: usize,
        elem: bool,
        set: usize,
        generate: bool,
    },
    Instance {
        inst: usize,
        got_translation: bool,
    },
    Density,
}

/// `cur` is the part receiving mesh data at the start of `text` (propagated
/// into `*INCLUDE`d files, which are textual inclusions).
fn parse_into(
    text: &str,
    dir: Option<&Path>,
    mesh: &mut Mesh,
    mut cur: Option<usize>,
) -> Result<(), Error> {
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
            target = keyword(mesh, dir, &mut cur, kw.trim(), params, n)?;
            continue;
        }
        if matches!(target, Target::Skip) {
            continue;
        }
        let full = logical(line, &mut lines);
        match target {
            Target::Skip => unreachable!(),
            Target::Node { part, nset } => {
                let mut fields = full.split(',').map(str::trim);
                let first = fields.next().unwrap_or("");
                if first.is_empty() {
                    continue;
                }
                let id = parse_id(first, n)?;
                let mut coords = [0.0; 3];
                for (c, f) in coords.iter_mut().zip(fields) {
                    if !f.is_empty() {
                        *c = parse_f64(f, n)?;
                    }
                }
                let p = &mut mesh.parts[part];
                p.node_ids.push(id);
                p.node_coords.push(coords);
                if let Some(s) = nset {
                    p.node_sets[s].ids.push(id);
                }
            }
            Target::Element { part, block, elset } => {
                let mut fields = full.split(',').map(str::trim).filter(|f| !f.is_empty());
                let Some(first) = fields.next() else {
                    continue;
                };
                let id = parse_id(first, n)?;
                let p = &mut mesh.parts[part];
                let b = &mut p.element_blocks[block];
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
                    p.element_sets[s].ids.push(id);
                }
            }
            Target::Set {
                part,
                elem,
                set,
                generate,
            } => {
                let p = &mut mesh.parts[part];
                let sets = if elem {
                    &mut p.element_sets
                } else {
                    &mut p.node_sets
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
            Target::Instance {
                inst,
                got_translation,
            } => {
                let mut vals = [0.0; 7];
                let mut count = 0;
                for f in full.split(',').map(str::trim).filter(|f| !f.is_empty()) {
                    if count == vals.len() {
                        return Err(err(n, "too many instance transform values"));
                    }
                    vals[count] = parse_f64(f, n)?;
                    count += 1;
                }
                let i = &mut mesh.instances[inst];
                if !got_translation {
                    if count != 3 {
                        return Err(err(n, "instance translation needs 3 values"));
                    }
                    i.translation = [vals[0], vals[1], vals[2]];
                    target = Target::Instance {
                        inst,
                        got_translation: true,
                    };
                } else if count == 7 && i.rotation.is_none() {
                    i.rotation = Some(Rotation {
                        axis_a: [vals[0], vals[1], vals[2]],
                        axis_b: [vals[3], vals[4], vals[5]],
                        angle_deg: vals[6],
                    });
                } else {
                    return Err(err(n, "instance rotation needs 7 values"));
                }
            }
            Target::Density => {
                let f = full.split(',').map(str::trim).next().unwrap_or("");
                if !f.is_empty() {
                    // Density is per material; keyword() guaranteed one exists.
                    mesh.materials.last_mut().unwrap().density = Some(parse_f64(f, n)?);
                }
                // Further data lines (temperature dependence) are not needed.
                target = Target::Skip;
            }
        }
    }
    Ok(())
}

/// Handle a keyword line; returns what the following data lines mean.
fn keyword(
    mesh: &mut Mesh,
    dir: Option<&Path>,
    cur: &mut Option<usize>,
    kw: &str,
    params: &str,
    n: usize,
) -> Result<Target, Error> {
    if kw.eq_ignore_ascii_case("NODE") {
        let part = cur_part(mesh, cur);
        let nset = param(params, "NSET").map(|s| set_index(&mut mesh.parts[part].node_sets, s));
        Ok(Target::Node { part, nset })
    } else if kw.eq_ignore_ascii_case("ELEMENT") {
        let ty = param(params, "TYPE").ok_or_else(|| err(n, "*ELEMENT without TYPE"))?;
        let part = cur_part(mesh, cur);
        let elset =
            param(params, "ELSET").map(|s| set_index(&mut mesh.parts[part].element_sets, s));
        let p = &mut mesh.parts[part];
        p.element_blocks.push(ElementBlock {
            element_type: ty.to_owned(),
            ids: Vec::new(),
            connectivity: Vec::new(),
            nodes_per_element: 0,
        });
        Ok(Target::Element {
            part,
            block: p.element_blocks.len() - 1,
            elset,
        })
    } else if kw.eq_ignore_ascii_case("NSET") || kw.eq_ignore_ascii_case("ELSET") {
        let elem = kw.eq_ignore_ascii_case("ELSET");
        let key = if elem { "ELSET" } else { "NSET" };
        let name =
            param(params, key).ok_or_else(|| err(n, format!("*{key} without {key} name")))?;
        let part = match param(params, "INSTANCE") {
            Some(inst) => {
                let part_name = mesh
                    .instances
                    .iter()
                    .find(|i| i.name.eq_ignore_ascii_case(inst))
                    .ok_or_else(|| err(n, format!("unknown instance `{inst}`")))?
                    .part
                    .clone();
                part_index(mesh, &part_name)
            }
            None => cur_part(mesh, cur),
        };
        let p = &mut mesh.parts[part];
        let sets = if elem {
            &mut p.element_sets
        } else {
            &mut p.node_sets
        };
        Ok(Target::Set {
            part,
            elem,
            set: set_index(sets, name),
            generate: param(params, "GENERATE").is_some(),
        })
    } else if kw.eq_ignore_ascii_case("PART") {
        let name = param(params, "NAME").ok_or_else(|| err(n, "*PART without NAME"))?;
        *cur = Some(part_index(mesh, name));
        Ok(Target::Skip)
    } else if kw.eq_ignore_ascii_case("INSTANCE") {
        let name = param(params, "NAME").ok_or_else(|| err(n, "*INSTANCE without NAME"))?;
        let part = param(params, "PART").ok_or_else(|| err(n, "*INSTANCE without PART"))?;
        // Mesh data inside the instance block belongs to the instanced part.
        *cur = Some(part_index(mesh, part));
        mesh.instances.push(Instance {
            name: name.to_owned(),
            part: part.to_owned(),
            translation: [0.0; 3],
            rotation: None,
        });
        Ok(Target::Instance {
            inst: mesh.instances.len() - 1,
            got_translation: false,
        })
    } else if kw.eq_ignore_ascii_case("END PART") || kw.eq_ignore_ascii_case("END INSTANCE") {
        *cur = None;
        Ok(Target::Skip)
    } else if kw.eq_ignore_ascii_case("MATERIAL") {
        let name = param(params, "NAME").ok_or_else(|| err(n, "*MATERIAL without NAME"))?;
        mesh.materials.push(Material {
            name: name.to_owned(),
            density: None,
        });
        Ok(Target::Skip)
    } else if kw.eq_ignore_ascii_case("DENSITY") {
        if mesh.materials.is_empty() {
            return Err(err(n, "*DENSITY outside *MATERIAL"));
        }
        Ok(Target::Density)
    } else if kw.eq_ignore_ascii_case("INCLUDE") {
        let input = param(params, "INPUT").ok_or_else(|| err(n, "*INCLUDE without INPUT"))?;
        let dir = dir.ok_or_else(|| err(n, "*INCLUDE needs parse_file (no base directory)"))?;
        let path = dir.join(input);
        let text =
            fs::read_to_string(&path).map_err(|e| err(n, format!("{}: {e}", path.display())))?;
        parse_into(&text, path.parent(), mesh, *cur).map_err(|e| Error {
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

/// Index of the named part, creating it if new (case-insensitive).
fn part_index(mesh: &mut Mesh, name: &str) -> usize {
    mesh.parts
        .iter()
        .position(|p| p.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| {
            mesh.parts.push(Part {
                name: name.to_owned(),
                ..Part::default()
            });
            mesh.parts.len() - 1
        })
}

/// The part receiving mesh data now; outside any part, the implicit `""` part.
fn cur_part(mesh: &mut Mesh, cur: &mut Option<usize>) -> usize {
    match *cur {
        Some(i) => i,
        None => {
            let i = part_index(mesh, "");
            *cur = Some(i);
            i
        }
    }
}

/// Index of the named set, creating it if new. Case-insensitive, so a set
/// name reused across blocks merges into one set.
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

fn parse_f64(f: &str, n: usize) -> Result<f64, Error> {
    f.parse().map_err(|_| err(n, format!("bad number `{f}`")))
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
        let p = &mesh.parts[0];
        assert_eq!(p.name, "");
        assert_eq!(p.node_ids, [1, 2]);
        assert_eq!(p.node_coords[1], [1.0, 1.0, 0.0]);
        let b = &p.element_blocks[0];
        assert_eq!(b.elements().collect::<Vec<_>>(), [(1, &[1, 2][..])]);
        assert_eq!(p.node_set("N").unwrap().ids, [1, 2]);
        assert_eq!(p.element_set("e").unwrap().ids, [1]);
    }

    #[test]
    fn skips_unknown_keywords() {
        let mesh = parse_str("*HEADING\nstuff, more\n*ELASTIC\n1., 2.\n*NODE\n1, 0.\n").unwrap();
        assert_eq!(mesh.parts[0].node_ids, [1]);
    }

    #[test]
    fn parts_have_local_numbering() {
        let mesh = parse_str(
            "*Part, name=A\n*Node\n1, 0., 0., 0.\n*End Part\n\
             *Part, name=B\n*Node\n1, 1., 0., 0.\n*End Part\n",
        )
        .unwrap();
        assert_eq!(mesh.parts.len(), 2);
        assert_eq!(mesh.part("a").unwrap().node_coords, [[0.0, 0.0, 0.0]]);
        assert_eq!(mesh.part("B").unwrap().node_coords, [[1.0, 0.0, 0.0]]);
    }

    #[test]
    fn instance_transforms() {
        let mesh = parse_str(
            "*Part, name=P\n*Node\n1, 0., 0., 0.\n*End Part\n*Assembly, name=A\n\
             *Instance, name=P-1, part=P\n*End Instance\n\
             *Instance, name=P-2, part=P\n1., 2., 3.\n*End Instance\n\
             *Instance, name=P-3, part=P\n0., 0., 0.\n0., 0., 0., 0., 0., 1., 90.\n*End Instance\n\
             *End Assembly\n",
        )
        .unwrap();
        assert_eq!(mesh.instances.len(), 3);
        assert_eq!(mesh.instances[0].translation, [0.0; 3]);
        assert_eq!(mesh.instances[1].translation, [1.0, 2.0, 3.0]);
        let rot = mesh.instances[2].rotation.unwrap();
        assert_eq!((rot.axis_b, rot.angle_deg), ([0.0, 0.0, 1.0], 90.0));
        assert!(mesh.instances[1].rotation.is_none());
    }

    #[test]
    fn materials_and_density() {
        let mesh = parse_str(
            "*Material, name=Steel_1\n*Elastic\n210000., 0.3\n\
             *Material, name=Water_2\n*Density\n 1.0,\n",
        )
        .unwrap();
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(mesh.materials[0].density, None);
        assert_eq!(mesh.materials[1].density, Some(1.0));
        assert!(parse_str("*Density\n1.0,\n").is_err());
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
