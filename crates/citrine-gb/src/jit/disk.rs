use super::emit::Emitter;
use super::helpers::Helpers;
use super::native;
use super::worker::{Job, Scan};
use super::{Jit, JitBus};
use crate::jit_cache::cpu_tag;

impl Jit {
    pub(super) fn job<const V5: bool, B: JitBus>(&self, scan: Scan) -> Job {
        let table: Box<[usize]> = Helpers::of::<V5, B>().table().into();
        let name = std::any::type_name::<B>();
        let cpu = match self.native {
            Some(_) => cpu_tag(&format!("{name} native"), V5),
            None => cpu_tag(name, V5),
        };
        Job {
            scan,
            tag: self.tag,
            emit: match self.native {
                Some(_) => native::emit::<V5, B>,
                None => Emitter::emit::<V5, B>,
            },
            table,
            disk: self.disk.clone().map(|disk| (disk, cpu)),
        }
    }
}
