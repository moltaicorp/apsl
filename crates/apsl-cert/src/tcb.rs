
use apsl_core::canon::{write_str, ArrayWriter, Canon, ObjectWriter};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TcbManifest {
    pub components: Vec<(String, String, String)>,
}

impl Canon for TcbManifest {
    fn write_canon(&self, out: &mut String) {
        let mut sorted = self.components.clone();
        sorted.sort();
        let mut aw = ArrayWriter::new(out);
        for (n, h, v) in &sorted {
            aw.item(|o| {
                let mut ow = ObjectWriter::new(o);
                ow.field("h", |o2| write_str(o2, h));
                ow.field("n", |o2| write_str(o2, n));
                ow.field("v", |o2| write_str(o2, v));
                ow.finish();
            });
        }
        aw.finish();
    }
}

impl TcbManifest {
    pub fn add(&mut self, name: impl Into<String>, hash: impl Into<String>, version: impl Into<String>) {
        self.components.push((name.into(), hash.into(), version.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_manifest_canon() {
        assert_eq!(TcbManifest::default().canon(), "[]");
    }

    #[test]
    fn manifest_components_sort() {
        let mut m = TcbManifest::default();
        m.add("z3", "aaa", "4.13");
        m.add("apsl-core", "bbb", "0.1.0");
        let s = m.canon();
        assert!(s.find("apsl-core").unwrap() < s.find("z3").unwrap());
    }
}
