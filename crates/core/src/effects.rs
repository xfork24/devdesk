use super::*;
impl Engine {
    pub(super) fn observe_effect(&self, id: &str, receipt: Value, confirmed: bool) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let e = s
                .effects
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| err("not_found", "外部操作不存在"))?;
            if [EffectStatus::Applied, EffectStatus::Discarded].contains(&e.status) {
                return Ok(json!({}));
            }
            if e.status == EffectStatus::Observed {
                require(e.receipt.as_ref() == Some(&receipt), "重复外部终态回执冲突")?;
                return Ok(json!({}));
            }
            if confirmed {
                e.receipt = Some(receipt.clone());
                e.status = EffectStatus::Observed;
            }
            e.updated = now();
            e.revision += 1;
            event(
                s,
                id,
                if confirmed {
                    "effect.observed"
                } else {
                    "effect.unconfirmed"
                },
                &receipt.to_string(),
            );
            Ok(json!({}))
        })?;
        Ok(())
    }
    pub(super) fn finish_effects(
        &self,
        s: &mut EngineeringState,
        run: &str,
        status: EffectStatus,
        reason: &str,
    ) {
        for e in s.effects.iter_mut().filter(|e| {
            e.run_id == run
                && e.kind != EffectKind::Cleanup
                && [EffectStatus::Pending, EffectStatus::Observed].contains(&e.status)
        }) {
            e.status = status;
            e.reason = reason.into();
            e.updated = now();
            e.revision += 1;
        }
    }
}
