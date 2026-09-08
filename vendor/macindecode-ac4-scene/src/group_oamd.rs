//! 所选 Full A-JOC group 的 OAMD common/timing 事务状态。

use macindecode_ac4_bitstream::{
    oamd::{
        OamdCommonData, OamdContext, OamdState, OamdSubstreamPayload, OamdTimingData,
        ObjectDescriptors,
    },
    substream::SubstreamInfo,
    topology::{Ac4Topology, MAX_SUBSTREAM_GROUPS, MAX_SUBSTREAMS},
};

use crate::{
    BitstreamFailure, DecodeError, DecodeErrorContext, DecodeErrorKind, DecodeStage,
    UnsupportedReason,
};

const OAMD_SYNTAX: &str = "raw_ac4_frame/ac4_substream/oamd_substream";
const GROUP_SYNTAX: &str = "raw_ac4_frame/ac4_toc/ac4_substream_group_info";

/// 一帧中一个所选 group 的有效 OAMD 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreparedGroupOamdState {
    pub(crate) group_index: u32,
    pub(crate) effective_common: Option<OamdCommonData>,
    pub(crate) common_updated_in_source_access_unit: bool,
    pub(crate) effective_timing: Option<OamdTimingData>,
    pub(crate) timing_updated_in_source_access_unit: bool,
    pub(crate) content_classifier: Option<u8>,
}

impl PreparedGroupOamdState {
    const EMPTY: Self = Self {
        group_index: 0,
        effective_common: None,
        common_updated_in_source_access_unit: false,
        effective_timing: None,
        timing_updated_in_source_access_unit: false,
        content_classifier: None,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalOamdUpdate {
    substream_index: u32,
    state: OamdState,
    current_common: Option<OamdCommonData>,
    timing_updated: bool,
}

#[derive(Debug, Clone, Copy)]
struct PhysicalOamdRequest<'a> {
    raw_frame: &'a [u8],
    topology: &'a Ac4Topology,
    group: &'a macindecode_ac4_bitstream::substream::Ac4SubstreamGroupInfo,
    group_index: u32,
    substream_index: u32,
    ndot: bool,
    reset_history: bool,
    scope: ErrorScope,
}

impl PhysicalOamdUpdate {
    const EMPTY: Self = Self {
        substream_index: 0,
        state: OamdState::new(),
        current_common: None,
        timing_updated: false,
    };
}

/// 尚未提交的 group OAMD 候选；只保存本 presentation 实际引用的固定容量条目。
#[derive(Debug)]
pub(crate) struct PreparedGroupOamd {
    reset_history: bool,
    physical_updates: [PhysicalOamdUpdate; MAX_SUBSTREAM_GROUPS],
    physical_updates_written: usize,
    groups: [PreparedGroupOamdState; MAX_SUBSTREAM_GROUPS],
    groups_written: usize,
    group_num_obj_info_blocks: Option<u8>,
    alternative: bool,
}

impl PreparedGroupOamd {
    fn new(reset_history: bool) -> Self {
        Self {
            reset_history,
            physical_updates: [PhysicalOamdUpdate::EMPTY; MAX_SUBSTREAM_GROUPS],
            physical_updates_written: 0,
            groups: [PreparedGroupOamdState::EMPTY; MAX_SUBSTREAM_GROUPS],
            groups_written: 0,
            group_num_obj_info_blocks: None,
            alternative: false,
        }
    }

    #[must_use]
    pub(crate) fn groups(&self) -> &[PreparedGroupOamdState] {
        self.groups.get(..self.groups_written).unwrap_or(&[])
    }

    #[must_use]
    #[cfg_attr(
        all(not(feature = "audio-decode"), not(test)),
        expect(
            dead_code,
            reason = "只有 audio-decode 的 Full engine 输入需要有效 OAMD 块数"
        )
    )]
    pub(crate) const fn group_num_obj_info_blocks(&self) -> Option<u8> {
        self.group_num_obj_info_blocks
    }

    #[must_use]
    #[cfg_attr(
        all(not(feature = "audio-decode"), not(test)),
        expect(
            dead_code,
            reason = "只有 audio-decode 的 Full engine 输入需要 alternative 标志"
        )
    )]
    pub(crate) const fn alternative(&self) -> bool {
        self.alternative
    }

    fn physical_update(&self, substream_index: u32) -> Option<PhysicalOamdUpdate> {
        self.physical_updates
            .iter()
            .take(self.physical_updates_written)
            .find(|update| update.substream_index == substream_index)
            .copied()
    }

    fn push_physical(
        &mut self,
        update: PhysicalOamdUpdate,
        scope: ErrorScope,
        group_index: u32,
    ) -> Result<(), DecodeError> {
        let Some(slot) = self.physical_updates.get_mut(self.physical_updates_written) else {
            return Err(internal_error(
                scope,
                group_index,
                Some(update.substream_index),
            ));
        };
        *slot = update;
        self.physical_updates_written = self.physical_updates_written.saturating_add(1);
        Ok(())
    }

    fn push_group(
        &mut self,
        state: PreparedGroupOamdState,
        scope: ErrorScope,
    ) -> Result<(), DecodeError> {
        let Some(slot) = self.groups.get_mut(self.groups_written) else {
            return Err(internal_error(scope, state.group_index, None));
        };
        *slot = state;
        self.groups_written = self.groups_written.saturating_add(1);
        Ok(())
    }

    fn merge_block_count(
        &mut self,
        count: Option<u8>,
        scope: ErrorScope,
        group_index: u32,
    ) -> Result<(), DecodeError> {
        let Some(next) = count else {
            return Ok(());
        };
        match self.group_num_obj_info_blocks {
            None => self.group_num_obj_info_blocks = Some(next),
            Some(current) if current == next => {}
            Some(current) => {
                return Err(DecodeError::new(
                    DecodeErrorKind::InvalidBitstream(BitstreamFailure::OamdTimingConflict {
                        expected: current,
                        actual: next,
                    }),
                    scoped_context(scope, group_index, None).with_syntax_path(OAMD_SYNTAX),
                ));
            }
        }
        Ok(())
    }
}

/// 已提交的 group OAMD 历史。物理 substream timing 与逐 group common 分开持有。
#[derive(Debug)]
pub(crate) struct GroupOamdDecoder {
    physical: [OamdState; MAX_SUBSTREAMS],
    group_common: [Option<OamdCommonData>; MAX_SUBSTREAM_GROUPS],
}

impl GroupOamdDecoder {
    pub(crate) const fn new() -> Self {
        Self {
            physical: [OamdState::new(); MAX_SUBSTREAMS],
            group_common: [None; MAX_SUBSTREAM_GROUPS],
        }
    }

    pub(crate) fn reset(&mut self) {
        for state in &mut self.physical {
            state.reset();
        }
        self.group_common.fill(None);
    }

    /// 在副本上解析当前 presentation 的 group common/timing；任何失败都不改写历史。
    pub(crate) fn prepare(
        &self,
        raw_frame: &[u8],
        topology: &Ac4Topology,
        group_mask: u8,
        reset_history: bool,
        scope: ErrorScope,
    ) -> Result<PreparedGroupOamd, DecodeError> {
        let mut prepared = PreparedGroupOamd::new(reset_history);

        for (group_position, group) in topology.groups().iter().enumerate() {
            let group_index = u32::try_from(group_position).unwrap_or(u32::MAX);
            let Some(group_bit) = 1u8.checked_shl(group_index) else {
                return Err(internal_error(scope, group_index, None));
            };
            if group_mask & group_bit == 0 {
                continue;
            }

            prepared.alternative |= group_is_alternative(topology, group_index);
            let external = match group.oamd_substream {
                Some(reference) => {
                    let Some(substream_index) = reference.substream_index else {
                        return Err(DecodeError::new(
                            DecodeErrorKind::Unsupported(
                                UnsupportedReason::OamdSubstreamIndexAbsent,
                            ),
                            scoped_context(scope, group_index, None).with_syntax_path(GROUP_SYNTAX),
                        ));
                    };
                    match prepared.physical_update(substream_index) {
                        Some(update) => Some(update),
                        None => {
                            let update = self.prepare_physical(PhysicalOamdRequest {
                                raw_frame,
                                topology,
                                group,
                                group_index,
                                substream_index,
                                ndot: reference.ndot,
                                reset_history,
                                scope,
                            })?;
                            prepared.push_physical(update, scope, group_index)?;
                            Some(update)
                        }
                    }
                }
                None => None,
            };

            let mut inline_current = None;
            for info in group.substreams() {
                let SubstreamInfo::Ajoc(info) = *info else {
                    continue;
                };
                if let Some(common) = info.oamd_common_data {
                    merge_current_common(
                        &mut inline_current,
                        common,
                        scope,
                        group_index,
                        info.substream_index(),
                    )?;
                }
            }
            if let (Some(inline), Some(external)) = (
                inline_current,
                external.and_then(|update| update.current_common),
            ) && inline != external
            {
                return Err(common_conflict(
                    scope,
                    group_index,
                    group.oamd_substream.and_then(|item| item.substream_index),
                ));
            }

            let current_common =
                inline_current.or_else(|| external.and_then(|update| update.current_common));
            let history = if reset_history {
                None
            } else {
                self.group_common.get(group_position).copied().flatten()
            };
            let effective_timing = external.and_then(|update| update.state.effective_timing());
            prepared.merge_block_count(
                effective_timing.map(|timing| timing.num_obj_info_blocks),
                scope,
                group_index,
            )?;
            prepared.push_group(
                PreparedGroupOamdState {
                    group_index,
                    effective_common: current_common.or(history),
                    common_updated_in_source_access_unit: current_common.is_some(),
                    effective_timing,
                    timing_updated_in_source_access_unit: external
                        .is_some_and(|update| update.timing_updated),
                    content_classifier: group
                        .content_type
                        .map(|content| content.content_classifier),
                },
                scope,
            )?;
        }

        Ok(prepared)
    }

    fn prepare_physical(
        &self,
        request: PhysicalOamdRequest<'_>,
    ) -> Result<PhysicalOamdUpdate, DecodeError> {
        let PhysicalOamdRequest {
            raw_frame,
            topology,
            group,
            group_index,
            substream_index,
            ndot,
            reset_history,
            scope,
        } = request;
        let position = usize::try_from(substream_index).unwrap_or(usize::MAX);
        let Some(committed) = self.physical.get(position).copied() else {
            return Err(internal_error(scope, group_index, Some(substream_index)));
        };
        let mut state = if reset_history {
            OamdState::new()
        } else {
            committed
        };
        let payload = topology
            .substream_payload(raw_frame, substream_index)
            .map_err(|error| {
                DecodeError::new(
                    DecodeErrorKind::InvalidBitstream(BitstreamFailure::Topology(error)),
                    scoped_context(scope, group_index, Some(substream_index))
                        .with_syntax_path(OAMD_SYNTAX),
                )
            })?;
        let descriptors = ObjectDescriptors::from_group(group).map_err(|error| {
            DecodeError::new(
                DecodeErrorKind::InvalidBitstream(BitstreamFailure::Oamd(error)),
                scoped_context(scope, group_index, Some(substream_index))
                    .with_syntax_path(GROUP_SYNTAX),
            )
        })?;
        let parsed = OamdSubstreamPayload::parse(
            payload,
            OamdContext {
                objects: descriptors.as_slice(),
                b_alternative: group_is_alternative(topology, group_index),
                b_oamd_ndot: ndot,
                previous_num_obj_info_blocks: state.previous_num_obj_info_blocks(),
            },
        )
        .map_err(|error| {
            DecodeError::new(
                DecodeErrorKind::InvalidBitstream(BitstreamFailure::Oamd(error)),
                scoped_context(scope, group_index, Some(substream_index))
                    .with_syntax_path(OAMD_SYNTAX),
            )
        })?;
        state.apply(&parsed).map_err(|error| {
            DecodeError::new(
                DecodeErrorKind::InvalidBitstream(BitstreamFailure::OamdState(error)),
                scoped_context(scope, group_index, Some(substream_index))
                    .with_syntax_path(OAMD_SYNTAX),
            )
        })?;

        Ok(PhysicalOamdUpdate {
            substream_index,
            state,
            current_common: parsed.common,
            timing_updated: parsed.timing.is_some(),
        })
    }

    pub(crate) fn commit(&mut self, prepared: &PreparedGroupOamd) {
        if prepared.reset_history {
            self.reset();
        }
        for update in prepared
            .physical_updates
            .iter()
            .take(prepared.physical_updates_written)
        {
            let position = usize::try_from(update.substream_index).unwrap_or(usize::MAX);
            if let Some(slot) = self.physical.get_mut(position) {
                *slot = update.state;
            }
        }
        for group in prepared.groups() {
            if !group.common_updated_in_source_access_unit {
                continue;
            }
            let position = usize::try_from(group.group_index).unwrap_or(usize::MAX);
            if let Some(slot) = self.group_common.get_mut(position) {
                *slot = group.effective_common;
            }
        }
    }
}

impl Default for GroupOamdDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ErrorScope {
    pub(crate) access_unit_index: u64,
    pub(crate) presentation_index: u32,
    pub(crate) presentation_id: Option<u32>,
}

fn group_is_alternative(topology: &Ac4Topology, group_index: u32) -> bool {
    topology.presentations().iter().any(|presentation| {
        presentation
            .substream
            .is_some_and(|substream| substream.alternative)
            && presentation.group_indices().contains(&group_index)
    })
}

fn merge_current_common(
    current: &mut Option<OamdCommonData>,
    candidate: OamdCommonData,
    scope: ErrorScope,
    group_index: u32,
    substream_index: Option<u32>,
) -> Result<(), DecodeError> {
    match *current {
        Some(value) if value != candidate => {
            return Err(common_conflict(scope, group_index, substream_index));
        }
        None => *current = Some(candidate),
        _ => {}
    }
    Ok(())
}

fn scoped_context(
    scope: ErrorScope,
    group_index: u32,
    substream_index: Option<u32>,
) -> DecodeErrorContext {
    let mut context = DecodeErrorContext::for_access_unit(scope.access_unit_index)
        .with_presentation(scope.presentation_index, scope.presentation_id)
        .with_group(group_index);
    if let Some(index) = substream_index {
        context = context.with_substream(index);
    }
    context
}

fn common_conflict(
    scope: ErrorScope,
    group_index: u32,
    substream_index: Option<u32>,
) -> DecodeError {
    DecodeError::new(
        DecodeErrorKind::InvalidBitstream(BitstreamFailure::OamdCommonConflict),
        scoped_context(scope, group_index, substream_index).with_syntax_path(OAMD_SYNTAX),
    )
}

fn internal_error(
    scope: ErrorScope,
    group_index: u32,
    substream_index: Option<u32>,
) -> DecodeError {
    DecodeError::new(
        DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::Oamd,
        },
        scoped_context(scope, group_index, substream_index).with_syntax_path(OAMD_SYNTAX),
    )
}

#[cfg(test)]
mod tests {
    use alloc::{format, vec, vec::Vec};

    use super::*;

    const TOC_PREFIX: &str = "10 0000000000 0 1 1101 1 1 0 0";
    const PRESENTATION_NDOT: &str = "1 0 000 0 00 000 0 00 00 0 000 0 0 0 1 00";
    // presentation substream 0、A-JOC audio 1、OAMD 2。
    const FULL_AJOC_GROUP_WITH_OAMD: &str = "1 0 1 0 1 1 10 1 1 0 1000 1 0 0011 1 0 0 1 01 0";

    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "测试位串按已计数长度写入新分配的字节数组"
    )]
    fn pack_bits(parts: &[&str]) -> Vec<u8> {
        let bits = parts
            .iter()
            .flat_map(|part| part.bytes())
            .filter(|bit| matches!(*bit, b'0' | b'1'))
            .collect::<Vec<_>>();
        let mut bytes = vec![0u8; bits.len().div_ceil(8)];
        for (index, bit) in bits.into_iter().enumerate() {
            if bit == b'1' {
                bytes[index / 8] |= 1 << (7 - index % 8);
            }
        }
        bytes
    }

    fn frame_with_oamd_payload(payload: &[u8]) -> (Vec<u8>, Ac4Topology) {
        let size = format!("{size:010b}", size = payload.len());
        let table = ["11 0 0000000000 0 0000000000 0 ", &size].concat();
        let mut frame = pack_bits(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP_WITH_OAMD,
            &table,
        ]);
        frame.extend_from_slice(payload);
        let topology = Ac4Topology::parse(&frame).expect("测试 topology 应合法");
        (frame, topology)
    }

    fn scope(index: u64) -> ErrorScope {
        ErrorScope {
            access_unit_index: index,
            presentation_index: 0,
            presentation_id: None,
        }
    }

    #[test]
    fn common_history_is_transactional_and_inherits_only_after_commit() {
        // outer common-present=1；common 使用 default screen ratio，其余关闭；无 timing。
        let (common_frame, common_topology) = frame_with_oamd_payload(&[0b1100_0000]);
        let (reuse_frame, reuse_topology) = frame_with_oamd_payload(&[0]);
        let mut decoder = GroupOamdDecoder::new();

        let first = decoder
            .prepare(&common_frame, &common_topology, 1, true, scope(0))
            .expect("首帧 common 应可形成候选");
        let first_group = first.groups().first().expect("应有 group 0");
        assert!(first_group.effective_common.is_some());
        assert!(first_group.common_updated_in_source_access_unit);

        let before_commit = decoder
            .prepare(&reuse_frame, &reuse_topology, 1, false, scope(1))
            .expect("未提交候选不应污染下一事务");
        let before_commit_group = before_commit.groups().first().expect("应有 group 0");
        assert_eq!(before_commit_group.effective_common, None);

        decoder.commit(&first);
        let inherited = decoder
            .prepare(&reuse_frame, &reuse_topology, 1, false, scope(1))
            .expect("提交后应继承 common");
        let inherited_group = inherited.groups().first().expect("应有 group 0");
        assert_eq!(
            inherited_group.effective_common,
            first_group.effective_common
        );
        assert!(!inherited_group.common_updated_in_source_access_unit);

        let reset = decoder
            .prepare(&reuse_frame, &reuse_topology, 1, true, scope(2))
            .expect("reset 候选应从空历史解析");
        assert_eq!(
            reset.groups().first().map(|group| group.effective_common),
            Some(None)
        );
    }

    #[test]
    fn timing_is_exposed_with_raw_refresh_provenance() {
        // outer common=0、timing=1；implicit offset、一个零偏移/零 ramp block。
        let timing_payload = pack_bits(&["0 1 0 001 000000 00"]);
        let (frame, topology) = frame_with_oamd_payload(&timing_payload);
        let decoder = GroupOamdDecoder::new();

        let prepared = decoder
            .prepare(&frame, &topology, 1, true, scope(7))
            .expect("显式 timing 应可解析");
        let group = prepared.groups().first().expect("应有 group 0");
        assert_eq!(prepared.group_num_obj_info_blocks(), Some(1));
        assert!(!prepared.alternative());
        assert!(group.timing_updated_in_source_access_unit);
        assert_eq!(
            group
                .effective_timing
                .map(|timing| timing.num_obj_info_blocks),
            Some(1)
        );
    }

    #[test]
    fn conflicting_common_values_fail_with_group_and_substream_context() {
        let mut current = Some(OamdCommonData {
            default_screen_size_ratio: true,
            master_screen_size_ratio_code: None,
            bed_object_chan_distribute: false,
            add_data_bytes: None,
            trim: Default::default(),
            bed_render_info: Default::default(),
            headphone: Default::default(),
        });
        let error = merge_current_common(
            &mut current,
            OamdCommonData {
                default_screen_size_ratio: false,
                master_screen_size_ratio_code: Some(0),
                bed_object_chan_distribute: false,
                add_data_bytes: None,
                trim: Default::default(),
                bed_render_info: Default::default(),
                headphone: Default::default(),
            },
            scope(9),
            3,
            Some(6),
        )
        .expect_err("冲突 common 不得按来源优先级静默覆盖");

        assert_eq!(
            error.kind(),
            DecodeErrorKind::InvalidBitstream(BitstreamFailure::OamdCommonConflict)
        );
        assert_eq!(error.context().presentation_index(), Some(0));
        assert_eq!(error.context().group_index(), Some(3));
        assert_eq!(error.context().substream_index(), Some(6));
    }

    #[test]
    fn block_count_conflict_is_structured_and_transactional() {
        let mut prepared = PreparedGroupOamd::new(false);
        prepared
            .merge_block_count(Some(1), scope(4), 0)
            .expect("首个块数应建立候选");
        let error = prepared
            .merge_block_count(Some(2), scope(4), 1)
            .expect_err("所选 group 的块数冲突必须拒绝");

        assert_eq!(
            error.kind(),
            DecodeErrorKind::InvalidBitstream(BitstreamFailure::OamdTimingConflict {
                expected: 1,
                actual: 2,
            })
        );
        assert_eq!(prepared.group_num_obj_info_blocks(), Some(1));
    }
}
