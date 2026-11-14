use super::arena::relocate;
use super::emit::Emitter;
use super::helpers::Helpers;
use super::native;
use super::worker::{Done, Job, Outcome, Scan};
use super::{Jit, JitBus, key};
use crate::jit_cache::cpu_tag;
use std::time::Instant;

impl Jit {
    pub(super) fn job<const V5: bool, B: JitBus>(&self, scan: Scan) -> Result<Job, Box<Done>> {
        let began = Instant::now();
        let table: Box<[usize]> = Helpers::of::<V5, B>().table().into();
        let name = std::any::type_name::<B>();
        let cpu = match self.native {
            Some(_) => cpu_tag(&format!("{name} native"), V5),
            None => cpu_tag(name, V5),
        };
        let cached = self
            .disk
            .as_ref()
            .and_then(|disk| disk.find(cpu, key(scan.pc, scan.thumb), &scan.words, &scan.fetch));
        if !cfg!(feature = "jit-lockstep")
            && let Some(record) = &cached
            && let Some(bytes) = relocate(&record.code, &record.relocs, &table)
        {
            return Err(Box::new(Done {
                scan,
                tag: self.tag,
                outcome: Outcome::Code(bytes),
                nanos: began.elapsed().as_nanos() as u64,
                loaded: true,
                mismatched: false,
            }));
        }
        Ok(Job {
            scan,
            tag: self.tag,
            emit: match self.native {
                Some(_) => native::emit::<V5, B>,
                None => Emitter::emit::<V5, B>,
            },
            table,
            disk: self.disk.clone().map(|disk| (disk, cpu)),
            cached: cached.filter(|_| cfg!(feature = "jit-lockstep")),
        })
    }
}
