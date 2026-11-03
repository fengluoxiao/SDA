use core::fmt;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

// Browser Workers have no native thread-spawn facility. Keep identical channel
// order/PRNG advancement and DSP, running the two independent neural stages
// sequentially there. Native hosts retain the bounded decoder-local pool.
#[cfg(not(target_arch = "wasm32"))]
// Bounded source analysis counts the owner as a channel worker. Ordinary
// playback keeps its existing pool execution and worker budget.
struct NeuralPool {
    pool: Option<rayon::ThreadPool>,
    caller_participates: bool,
}
#[cfg(not(target_arch = "wasm32"))]
impl NeuralPool {
    fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        if self.caller_participates {
            f()
        } else {
            self.pool.as_ref().unwrap().install(f)
        }
    }
    fn for_each<T: Send>(&self, items: &mut [T], work: impl Fn(usize, &mut T) + Send + Sync) {
        if !self.caller_participates {
            items
                .par_iter_mut()
                .enumerate()
                .for_each(|(i, item)| work(i, item));
            return;
        }
        let Some(pool) = &self.pool else {
            for (i, item) in items.iter_mut().enumerate() {
                work(i, item);
            }
            return;
        };
        let mut items = items.iter_mut().enumerate();
        let first = items.next();
        let pending = std::sync::Mutex::new(items);
        let drain = || loop {
            let next = pending.lock().unwrap().next();
            match next {
                Some((i, item)) => work(i, item),
                None => break,
            }
        };
        pool.in_place_scope(|scope| {
            for _ in 0..pool.current_num_threads() {
                scope.spawn(|_| drain());
            }
            if let Some((i, item)) = first {
                work(i, item);
            }
            drain();
        });
    }
}
#[cfg(target_arch = "wasm32")]
struct NeuralPool;
#[cfg(target_arch = "wasm32")]
impl NeuralPool {
    fn install<R>(&self, f: impl FnOnce() -> R) -> R {
        f()
    }
    fn for_each<T>(&self, items: &mut [T], work: impl Fn(usize, &mut T)) {
        for (i, item) in items.iter_mut().enumerate() {
            work(i, item);
        }
    }
}

use crate::bwe::{BweSynthesis, BweSynthesisError};
use crate::core_side::{CoreSideInfo, ParsedNeuralQc};
use crate::fd_shaping::{FdShapingError, FdSpectrumShaping};
use crate::header::{FrameHeader, MAX_CHANNELS, NnType};
use crate::mc_side::{
    McBitstreamConfig, McError, McSideInfo, McSideInfoDecoder, clear_mc_lfe_spectrum,
    inverse_mc_coupling,
};
use crate::mdct_synthesis::{MdctSynthesis, MdctSynthesisError};
use crate::model::{AVS3_FEATURE_DIMENSIONS, NeuralModel};
use crate::neural_qc::{
    NeuralQcError, NeuralSpectrumDecoder, NeuralSpectrumDiagnostics, PreparedNeuralSpectrum,
};
use crate::random::Avs3Random;
use crate::spectrum::{SpectrumReorder, SpectrumReorderError};
use crate::tns::{TnsSynthesis, TnsSynthesisError};

pub const MC_MAX_FRAME_SAMPLES: usize = AVS3_FEATURE_DIMENSIONS * MAX_CHANNELS as usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McCoreDecodeError {
    ThreadPoolBuild,
    InvalidOutputLength { expected: usize, actual: usize },
    MissingCoreSideInformation { channel: usize },
    MissingNeuralSideInformation { channel: usize },
    UnexpectedNeuralProfile { channel: usize },
    InconsistentBweSideInformation { channel: usize },
    Mc(McError),
    NeuralQc(NeuralQcError),
    SpectrumReorder(SpectrumReorderError),
    Bwe(BweSynthesisError),
    Tns(TnsSynthesisError),
    FdShaping(FdShapingError),
    MdctSynthesis(MdctSynthesisError),
}

impl fmt::Display for McCoreDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThreadPoolBuild => f.write_str("failed to build MC neural worker pool"),
            Self::InvalidOutputLength { expected, actual } => write!(
                f,
                "multichannel synthesis output has {actual} samples; expected {expected}"
            ),
            Self::MissingCoreSideInformation { channel } => {
                write!(f, "MC channel {channel} is missing core side information")
            }
            Self::MissingNeuralSideInformation { channel } => {
                write!(f, "MC channel {channel} is missing neural side information")
            }
            Self::UnexpectedNeuralProfile { channel } => write!(
                f,
                "parsed neural QC profile does not match MC channel {channel} configuration"
            ),
            Self::InconsistentBweSideInformation { channel } => write!(
                f,
                "MC channel {channel} BWE configuration and side information are inconsistent"
            ),
            Self::Mc(error) => error.fmt(f),
            Self::NeuralQc(error) => error.fmt(f),
            Self::SpectrumReorder(error) => error.fmt(f),
            Self::Bwe(error) => error.fmt(f),
            Self::Tns(error) => error.fmt(f),
            Self::FdShaping(error) => error.fmt(f),
            Self::MdctSynthesis(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for McCoreDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Mc(error) => Some(error),
            Self::NeuralQc(error) => Some(error),
            Self::SpectrumReorder(error) => Some(error),
            Self::Bwe(error) => Some(error),
            Self::Tns(error) => Some(error),
            Self::FdShaping(error) => Some(error),
            Self::MdctSynthesis(error) => Some(error),
            _ => None,
        }
    }
}

impl From<McError> for McCoreDecodeError {
    fn from(value: McError) -> Self {
        Self::Mc(value)
    }
}

impl From<NeuralQcError> for McCoreDecodeError {
    fn from(value: NeuralQcError) -> Self {
        Self::NeuralQc(value)
    }
}

impl From<SpectrumReorderError> for McCoreDecodeError {
    fn from(value: SpectrumReorderError) -> Self {
        Self::SpectrumReorder(value)
    }
}

impl From<BweSynthesisError> for McCoreDecodeError {
    fn from(value: BweSynthesisError) -> Self {
        Self::Bwe(value)
    }
}

impl From<TnsSynthesisError> for McCoreDecodeError {
    fn from(value: TnsSynthesisError) -> Self {
        Self::Tns(value)
    }
}

impl From<FdShapingError> for McCoreDecodeError {
    fn from(value: FdShapingError) -> Self {
        Self::FdShaping(value)
    }
}

impl From<MdctSynthesisError> for McCoreDecodeError {
    fn from(value: MdctSynthesisError) -> Self {
        Self::MdctSynthesis(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McCoreDiagnostics {
    channels: usize,
    cores: [Option<CoreSideInfo>; MAX_CHANNELS as usize],
    mc: McSideInfo,
    neural: [Option<NeuralSpectrumDiagnostics>; MAX_CHANNELS as usize],
    entropy_bytes: [usize; MAX_CHANNELS as usize],
    consumed_bits: usize,
    padding_bits: usize,
}

impl McCoreDiagnostics {
    pub fn channels(self) -> usize {
        self.channels
    }

    pub fn core(self, channel: usize) -> Option<CoreSideInfo> {
        self.cores.get(channel).copied().flatten()
    }

    pub fn mc(self) -> McSideInfo {
        self.mc
    }

    pub fn neural(self, channel: usize) -> Option<NeuralSpectrumDiagnostics> {
        self.neural.get(channel).copied().flatten()
    }

    pub fn entropy_bytes(&self) -> &[usize] {
        &self.entropy_bytes[..self.channels]
    }

    pub fn consumed_bits(self) -> usize {
        self.consumed_bits
    }

    pub fn padding_bits(self) -> usize {
        self.padding_bits
    }
}

#[derive(Debug)]
struct McChannelDsp {
    reorder: SpectrumReorder,
    bwe: BweSynthesis,
    tns: TnsSynthesis,
    fd_shaping: FdSpectrumShaping,
    mdct_synthesis: MdctSynthesis,
}

impl McChannelDsp {
    fn new() -> Self {
        Self {
            reorder: SpectrumReorder::new(),
            bwe: BweSynthesis::new(),
            tns: TnsSynthesis::new(),
            fd_shaping: FdSpectrumShaping::new(),
            mdct_synthesis: MdctSynthesis::new(),
        }
    }

    fn reset(&mut self) {
        self.mdct_synthesis.reset();
    }
}

/// Channel-based multichannel payload-to-interleaved-floating-PCM decoder.
///
/// Neural workspaces are channel-local so entropy/context decoding can run in
/// parallel. Noise filling still consumes one PRNG in output-channel order,
/// exactly like the reference decoder. Construction performs all allocation.
pub struct McCoreDecoder<'model> {
    side_information: McSideInfoDecoder,
    neural_pool: NeuralPool,
    neural: Vec<NeuralSpectrumDecoder<'model>>,
    channel_dsp: Vec<McChannelDsp>,
    random: Avs3Random,
    spectra: Vec<[f32; AVS3_FEATURE_DIMENSIONS]>,
    synthesis: Vec<[f32; AVS3_FEATURE_DIMENSIONS]>,
    last_channels: usize,
}

impl<'model> McCoreDecoder<'model> {
    pub fn new(model: &'model NeuralModel) -> Result<Self, McCoreDecodeError> {
        Self::new_with_worker_limit(model, None)
    }
    fn new_with_worker_limit(
        model: &'model NeuralModel,
        worker_limit: Option<usize>,
    ) -> Result<Self, McCoreDecodeError> {
        #[cfg(target_arch = "wasm32")]
        let _ = worker_limit;
        let capacity = usize::from(MAX_CHANNELS);
        let mut channel_dsp = Vec::with_capacity(capacity);
        let mut neural = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            channel_dsp.push(McChannelDsp::new());
            neural.push(NeuralSpectrumDecoder::new(model)?);
        }
        #[cfg(target_arch = "wasm32")]
        let neural_pool = NeuralPool;
        #[cfg(not(target_arch = "wasm32"))]
        let neural_pool = {
            let workers = neural_worker_count()
                .min(worker_limit.unwrap_or(usize::MAX))
                .max(1);
            let caller_participates = worker_limit.is_some();
            let helpers = workers - usize::from(caller_participates);
            let pool = if helpers == 0 {
                None
            } else {
                Some(
                    rayon::ThreadPoolBuilder::new()
                        .num_threads(helpers)
                        .thread_name(|i| format!("avs3a-neural-{i}"))
                        .build()
                        .map_err(|_| McCoreDecodeError::ThreadPoolBuild)?,
                )
            };
            NeuralPool {
                pool,
                caller_participates,
            }
        };
        Ok(Self {
            side_information: McSideInfoDecoder::new(),
            neural_pool,
            neural,
            channel_dsp,
            random: Avs3Random::new(),
            spectra: vec![[0.0; AVS3_FEATURE_DIMENSIONS]; capacity],
            synthesis: vec![[0.0; AVS3_FEATURE_DIMENSIONS]; capacity],
            last_channels: 0,
        })
    }

    pub fn reset(&mut self) {
        self.random.reset();
        for channel in &mut self.channel_dsp {
            channel.reset();
        }
        for spectrum in &mut self.spectra {
            spectrum.fill(0.0);
        }
        for synthesis in &mut self.synthesis {
            synthesis.fill(0.0);
        }
        self.last_channels = 0;
    }

    pub fn random(&self) -> &Avs3Random {
        &self.random
    }

    pub fn random_mut(&mut self) -> &mut Avs3Random {
        &mut self.random
    }

    pub fn last_shaped_spectra(&self) -> &[[f32; AVS3_FEATURE_DIMENSIONS]] {
        &self.spectra[..self.last_channels]
    }

    pub fn decode(
        &mut self,
        payload: &[u8],
        header: &FrameHeader,
        output: &mut [f32],
    ) -> Result<McCoreDiagnostics, McCoreDecodeError> {
        let config = McBitstreamConfig::for_header(header)?;
        let channels = config.channels();
        let expected_output = channels
            .checked_mul(AVS3_FEATURE_DIMENSIONS)
            .ok_or(McError::AllocationOverflow)?;
        if output.len() != expected_output {
            return Err(McCoreDecodeError::InvalidOutputLength {
                expected: expected_output,
                actual: output.len(),
            });
        }

        let parsed = self.side_information.parse(payload, header)?;
        let mc = parsed.mc();
        let allocation = parsed.allocation();
        let consumed_bits = parsed.consumed_bits();
        let padding_bits = parsed.padding_bits();
        let mut frame_random = self.random.clone();
        let mut cores = [None; MAX_CHANNELS as usize];
        let mut neural_inputs = [None; MAX_CHANNELS as usize];
        let mut neural_diagnostics = [None; MAX_CHANNELS as usize];
        let mut entropy_bytes = [0_usize; MAX_CHANNELS as usize];
        entropy_bytes[..channels].copy_from_slice(allocation.channel_bytes());

        for channel in 0..channels {
            let core = parsed
                .core(channel)
                .ok_or(McCoreDecodeError::MissingCoreSideInformation { channel })?;
            let neural_qc = parsed
                .neural_qc(channel)
                .ok_or(McCoreDecodeError::MissingNeuralSideInformation { channel })?;
            cores[channel] = Some(core);
            neural_inputs[channel] = Some(neural_qc);
        }

        let mut prepared: [Option<Result<PreparedNeuralSpectrum<'_>, McCoreDecodeError>>;
            MAX_CHANNELS as usize] = core::array::from_fn(|_| None);
        let mut finish_errors: [Option<McCoreDecodeError>; MAX_CHANNELS as usize] =
            core::array::from_fn(|_| None);
        self.neural_pool
            .install(|| -> Result<(), McCoreDecodeError> {
                {
                    let mut iter = prepared[..channels]
                        .iter_mut()
                        .zip(self.neural[..channels].iter_mut())
                        .enumerate();
                    let mut jobs: [Option<_>; MAX_CHANNELS as usize] =
                        core::array::from_fn(|_| iter.next());
                    self.neural_pool.for_each(&mut jobs[..channels], |_, job| {
                        let (channel, (slot, decoder)) = job.as_mut().unwrap();
                        let result = match neural_inputs[*channel]
                            .expect("all active neural side information was validated")
                        {
                            ParsedNeuralQc::Main(input) if header.nn_type == NnType::Main => {
                                decoder.prepare_main(input).map_err(McCoreDecodeError::from)
                            }
                            ParsedNeuralQc::LowComplexity(input)
                                if header.nn_type == NnType::LowComplexity =>
                            {
                                decoder
                                    .prepare_low_complexity(input)
                                    .map_err(McCoreDecodeError::from)
                            }
                            _ => Err(McCoreDecodeError::UnexpectedNeuralProfile {
                                channel: *channel,
                            }),
                        };
                        **slot = Some(result);
                    });
                }

                let mut prepared_ok: [Option<PreparedNeuralSpectrum<'_>>; MAX_CHANNELS as usize] =
                    core::array::from_fn(|_| None);
                let mut channel_randoms: [Avs3Random; MAX_CHANNELS as usize] =
                    core::array::from_fn(|_| frame_random.clone());
                for channel in 0..channels {
                    let result = prepared[channel]
                        .take()
                        .expect("every active channel produced a preparation result")?;
                    let draws = self.neural[channel].noise_random_draws(result)?;
                    channel_randoms[channel] = frame_random.clone();
                    for _ in 0..draws {
                        frame_random.rand();
                    }
                    prepared_ok[channel] = Some(result);
                }

                {
                    let mut iter = prepared_ok[..channels]
                        .iter_mut()
                        .zip(self.neural[..channels].iter_mut())
                        .zip(self.spectra[..channels].iter_mut())
                        .zip(channel_randoms[..channels].iter_mut())
                        .zip(neural_diagnostics[..channels].iter_mut())
                        .zip(finish_errors[..channels].iter_mut());
                    let mut jobs: [Option<_>; MAX_CHANNELS as usize] =
                        core::array::from_fn(|_| iter.next());
                    self.neural_pool.for_each(&mut jobs[..channels], |_, job| {
                        let (((((prepared, decoder), spectrum), random), diagnostics), error) =
                            job.as_mut().unwrap();
                        match decoder
                            .finish_prepared(prepared.take().expect("prepared neural data"), random)
                        {
                            Ok(decoded) => {
                                **diagnostics = Some(decoded.diagnostics());
                                spectrum.copy_from_slice(decoded.spectrum());
                            }
                            Err(failure) => **error = Some(failure.into()),
                        }
                    });
                }
                if let Some(error) = finish_errors.iter_mut().find_map(Option::take) {
                    return Err(error);
                }

                for (channel, core) in cores[..channels].iter().copied().enumerate() {
                    let core =
                        core.ok_or(McCoreDecodeError::MissingCoreSideInformation { channel })?;
                    self.channel_dsp[channel].reorder.degroup(
                        core.grouping(),
                        core.transform_type(),
                        &mut self.spectra[channel],
                    )?;
                }

                inverse_mc_coupling(&mut self.spectra[..channels], mc, config)?;

                let bwe_config = config.core().bwe();
                for (channel, core) in cores[..channels].iter().copied().enumerate() {
                    let core =
                        core.ok_or(McCoreDecodeError::MissingCoreSideInformation { channel })?;
                    match (bwe_config, core.bwe()) {
                        (Some(bwe), Some(side_info)) => self.channel_dsp[channel].bwe.apply(
                            bwe,
                            side_info,
                            &mut self.spectra[channel],
                            &mut frame_random,
                        )?,
                        (None, None) => {}
                        _ => {
                            return Err(McCoreDecodeError::InconsistentBweSideInformation {
                                channel,
                            });
                        }
                    }
                }
                // BWE above consumes the normative shared PRNG in channel order.
                // These remaining stages own channel-local histories and can run on
                // the same bounded pool without changing any arithmetic or state order.
                let mut dsp_errors: [Option<McCoreDecodeError>; MAX_CHANNELS as usize] =
                    core::array::from_fn(|_| None);
                {
                    let mut iter = self.channel_dsp[..channels]
                        .iter_mut()
                        .zip(self.spectra[..channels].iter_mut())
                        .zip(self.synthesis[..channels].iter_mut())
                        .zip(dsp_errors[..channels].iter_mut())
                        .enumerate();
                    let mut jobs: [Option<_>; MAX_CHANNELS as usize] =
                        core::array::from_fn(|_| iter.next());
                    self.neural_pool.for_each(&mut jobs[..channels], |_, job| {
                        let (channel, (((dsp, spectrum), synthesis), error)) =
                            job.as_mut().unwrap();
                        let spectrum = &mut **spectrum;
                        let synthesis = &mut **synthesis;
                        let result = (|| -> Result<(), McCoreDecodeError> {
                            let core = cores[*channel].ok_or(
                                McCoreDecodeError::MissingCoreSideInformation { channel: *channel },
                            )?;
                            dsp.tns.apply(core.tns(), core.transform_type(), spectrum)?;
                            dsp.fd_shaping.apply(core.lsf(), spectrum)?;
                            if config.lfe_channel() == Some(*channel) {
                                clear_mc_lfe_spectrum(spectrum)?;
                            }
                            dsp.mdct_synthesis.synthesize(
                                spectrum,
                                core.transform_type(),
                                synthesis,
                            )?;
                            Ok(())
                        })();
                        **error = result.err();
                    });
                }
                if let Some(error) = dsp_errors.into_iter().flatten().next() {
                    return Err(error);
                }
                Ok(())
            })?;
        for sample in 0..AVS3_FEATURE_DIMENSIONS {
            for channel in 0..channels {
                output[sample * channels + channel] = self.synthesis[channel][sample];
            }
        }

        self.random = frame_random;
        self.last_channels = channels;
        Ok(McCoreDiagnostics {
            channels,
            cores,
            mc,
            neural: neural_diagnostics,
            entropy_bytes,
            consumed_bits,
            padding_bits,
        })
    }
}

impl McCoreDecoder<'static> {
    pub fn new_builtin() -> Result<Self, McCoreDecodeError> {
        let model = crate::builtin_model::builtin_neural_model().map_err(NeuralQcError::from)?;
        Self::new(model)
    }
    /// Bound total source-analysis channel workers, including the calling
    /// decoder thread, without changing global Rayon state or live playback.
    pub fn new_builtin_with_worker_limit(limit: usize) -> Result<Self, McCoreDecodeError> {
        let model = crate::builtin_model::builtin_neural_model().map_err(NeuralQcError::from)?;
        Self::new_with_worker_limit(model, Some(limit.max(1)))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn neural_worker_count() -> usize {
    std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&value| value != 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|value| value.get().div_ceil(2).min(8))
                .unwrap_or(1)
        })
}

impl fmt::Debug for McCoreDecoder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McCoreDecoder")
            .field("neural", &self.neural)
            .field("channel_states", &self.channel_dsp.len())
            .field("last_channels", &self.last_channels)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::BitWriter;
    use crate::header::{AudioCodecId, BitDepth, ChannelConfig, CodecProfile};

    fn header(payload_bits: usize) -> FrameHeader {
        FrameHeader {
            codec_id: AudioCodecId::Avs3P3,
            nn_type: NnType::Main,
            profile: CodecProfile::ChannelBased,
            sample_rate: 48_000,
            bit_depth: BitDepth::Sixteen,
            channel_config: Some(ChannelConfig::Mc5_1),
            sound_bed_type: None,
            hoa_order: None,
            objects: 0,
            bed_channels: 6,
            channels: 6,
            has_lfe: true,
            bed_bitrate: Some(384_000),
            object_bitrate: None,
            bitrate: 384_000,
            crc: 0,
            header_len: 7,
            payload_bits,
            payload_len: payload_bits.div_ceil(8),
            frame_len: 7 + payload_bits.div_ceil(8),
            samples_per_channel: AVS3_FEATURE_DIMENSIONS as u32,
        }
    }

    fn reference_payload() -> Vec<u8> {
        let mut writer = BitWriter::new();
        for _ in 0..6 {
            writer.write_bits(0, 2).unwrap();
            for width in [8, 8, 7, 7, 6, 5, 5] {
                writer.write_bits(0, width).unwrap();
            }
            writer.write_bits(0, 1).unwrap();
            writer.write_bits(0, 1).unwrap();
        }

        writer.write_bits(0, 1).unwrap();
        writer.write_bits(0, 4).unwrap();
        for ratio in [13, 13, 13, 13, 12] {
            writer.write_bits(ratio, 6).unwrap();
        }
        assert_eq!(writer.bit_len(), 335);

        for entropy_bytes in [64, 60, 60, 20, 60, 56] {
            write_qc(&mut writer, entropy_bytes);
        }
        assert_eq!(writer.bit_len(), 3_009);
        writer.into_bytes()
    }

    fn write_qc(writer: &mut BitWriter, entropy_bytes: usize) {
        let context: [u8; 6] = [0x84, 0xa0, 0xd8, 0x95, 0xb9, 0xa7];
        let base: [u8; 26] = [
            0x7f, 0xfd, 0x51, 0xf6, 0xf2, 0x24, 0x34, 0xad, 0x04, 0xde, 0x75, 0xcd, 0x9d, 0x0c,
            0x76, 0xeb, 0xb3, 0x76, 0xaf, 0x47, 0xda, 0x43, 0x33, 0xf0, 0xd4, 0xeb,
        ];
        writer.write_bits(1, 1).unwrap();
        writer.write_bits(37, 7).unwrap();
        writer.write_bits(3, 3).unwrap();
        writer.write_bits(context.len() as u64, 8).unwrap();
        for byte in context.into_iter().chain(base).take(entropy_bytes) {
            writer.write_bits(u64::from(byte), 8).unwrap();
        }
        for _ in (context.len() + base.len()).min(entropy_bytes)..entropy_bytes {
            writer.write_bits(0, 8).unwrap();
        }
    }

    #[test]
    fn complete_six_channel_pipeline_decodes_without_frame_allocations() {
        let payload = reference_payload();
        let header = header(3_009);
        let mut decoder = McCoreDecoder::new_builtin().unwrap();
        let mut output = [0.0_f32; AVS3_FEATURE_DIMENSIONS * 6];
        let diagnostics = decoder.decode(&payload, &header, &mut output).unwrap();

        assert_eq!(diagnostics.channels(), 6);
        assert_eq!(diagnostics.entropy_bytes(), &[64, 60, 60, 20, 60, 56]);
        assert_eq!(diagnostics.consumed_bits(), 3_009);
        assert_eq!(diagnostics.padding_bits(), 0);
        assert!(output.iter().all(|sample| sample.is_finite()));
        let spectra = decoder.last_shaped_spectra();
        assert!(
            spectra[3][crate::mc_side::MC_LFE_RESERVED_LINES..]
                .iter()
                .all(|&value| value == 0.0)
        );
    }

    #[test]
    fn invalid_output_length_does_not_advance_random_state() {
        let payload = reference_payload();
        let header = header(3_009);
        let mut decoder = McCoreDecoder::new_builtin().unwrap();
        let original_random = decoder.random().clone();
        let mut output = [7.0_f32; 17];
        let error = decoder.decode(&payload, &header, &mut output).unwrap_err();
        assert_eq!(
            error,
            McCoreDecodeError::InvalidOutputLength {
                expected: AVS3_FEATURE_DIMENSIONS * 6,
                actual: 17,
            }
        );
        assert_eq!(decoder.random(), &original_random);
        assert_eq!(output, [7.0; 17]);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod caller_pool_tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    fn pool(workers: usize, caller: bool) -> NeuralPool {
        let helpers = workers - usize::from(caller);
        NeuralPool {
            pool: if helpers == 0 {
                None
            } else {
                Some(
                    rayon::ThreadPoolBuilder::new()
                        .num_threads(helpers)
                        .build()
                        .unwrap(),
                )
            },
            caller_participates: caller,
        }
    }
    #[test]
    fn bounded_analysis_executes_on_owner_and_visits_every_channel_once() {
        let owner = std::thread::current().id();
        let original_index = rayon::current_thread_index();
        for workers in 1..=4 {
            let pool = pool(workers, true);
            for count in [0, 1, 2, 16, MAX_CHANNELS as usize] {
                let mut values = vec![0usize; count];
                let calls: Vec<_> = (0..count).map(|_| AtomicUsize::new(0)).collect();
                let owner_used = AtomicBool::new(false);
                let threads = Mutex::new(std::collections::HashSet::new());
                pool.install(|| {
                    pool.for_each(&mut values, |i, value| {
                        calls[i].fetch_add(1, Ordering::Relaxed);
                        *value = i * 17 + 9;
                        let current = std::thread::current().id();
                        threads.lock().unwrap().insert(current);
                        if current == owner {
                            owner_used.store(true, Ordering::Relaxed);
                        }
                    })
                });
                assert_eq!(values, (0..count).map(|i| i * 17 + 9).collect::<Vec<_>>());
                assert!(calls.iter().all(|n| n.load(Ordering::Relaxed) == 1));
                assert!(count == 0 || owner_used.load(Ordering::Relaxed));
                assert!(threads.lock().unwrap().len() <= workers);
                assert_eq!(rayon::current_thread_index(), original_index);
            }
        }
    }
    #[test]
    fn normal_playback_keeps_work_on_the_decoder_pool() {
        let pool = pool(3, false);
        let owner = std::thread::current().id();
        let mut values = [0usize; 32];
        pool.install(|| {
            pool.for_each(&mut values, |i, value| {
                assert_ne!(std::thread::current().id(), owner);
                assert!(rayon::current_thread_index().is_some());
                *value = i + 1;
            })
        });
        assert_eq!(values, core::array::from_fn(|i| i + 1));
    }
    #[test]
    fn bounded_pools_can_be_recreated_on_one_owner_without_registering_it() {
        let before = rayon::current_thread_index();
        for _ in 0..24 {
            let pool = pool(3, true);
            let mut values = [0; 24];
            pool.install(|| pool.for_each(&mut values, |i, value| *value = i));
            assert_eq!(rayon::current_thread_index(), before);
        }
    }
}
