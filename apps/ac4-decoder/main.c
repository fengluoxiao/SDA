/* Diagnostic AC-4 stereo export using the LGPL Librempeg libraries. */
#include <errno.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <sys/stat.h>
#ifdef _WIN32
#include <windows.h>
#include <io.h>
#endif
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/error.h>
#include <libavutil/log.h>
#include <libavutil/samplefmt.h>

static int audio_bounds_warning;
static void decoder_log(void *context, int level, const char *format, va_list args) {
    char message[1024];
    va_list copy;
    va_copy(copy, args);
    vsnprintf(message, sizeof(message), format, copy);
    va_end(copy);
    if (strstr(message, "substream audio data overread") ||
        strstr(message, "substream audio data underread")) audio_bounds_warning = 1;
    av_log_default_callback(context, level, format, args);
}

static void put16(uint8_t *dst, uint16_t value) {
    dst[0] = (uint8_t)value; dst[1] = (uint8_t)(value >> 8);
}
static void put32(uint8_t *dst, uint32_t value) {
    for (int i = 0; i < 4; i++) dst[i] = (uint8_t)(value >> (8 * i));
}
static int wave_header(FILE *output, int rate, uint64_t samples) {
    uint8_t header[56] = {0};
    if (samples > (UINT32_MAX - 48) / 8 || rate <= 0) return AVERROR(EINVAL);
    memcpy(header, "RIFF", 4); put32(header + 4, 48 + (uint32_t)samples * 8);
    memcpy(header + 8, "WAVEfmt ", 8); put32(header + 16, 16);
    put16(header + 20, 3); put16(header + 22, 2);
    put32(header + 24, rate); put32(header + 28, rate * 8);
    put16(header + 32, 8); put16(header + 34, 32);
    memcpy(header + 36, "fact", 4); put32(header + 40, 4);
    put32(header + 44, (uint32_t)samples);
    memcpy(header + 48, "data", 4); put32(header + 52, (uint32_t)samples * 8);
    if (fseek(output, 0, SEEK_SET) || fwrite(header, 1, sizeof(header), output) != sizeof(header))
        return AVERROR(EIO);
    return 0;
}

static int drain(AVCodecContext *decoder, AVFrame *frame, FILE *output,
                 int *rate, uint64_t *samples) {
    int result;
    while ((result = avcodec_receive_frame(decoder, frame)) >= 0) {
        if (frame->ch_layout.nb_channels != 2 || frame->format != AV_SAMPLE_FMT_FLTP ||
            (*rate && *rate != frame->sample_rate)) {
            av_frame_unref(frame);
            return AVERROR(ENOTSUP);
        }
        if (!*rate) {
            *rate = frame->sample_rate;
            if ((result = wave_header(output, *rate, 0)) < 0) return result;
        }
        if (*samples + frame->nb_samples > (UINT32_MAX - 48) / 8) return AVERROR(EFBIG);
        const float *left = (const float *)frame->extended_data[0];
        const float *right = (const float *)frame->extended_data[1];
        for (int i = 0; i < frame->nb_samples; i++) {
            uint32_t words[2];
            uint8_t pair[8];
            if (!isfinite(left[i]) || !isfinite(right[i])) return AVERROR_INVALIDDATA;
            memcpy(&words[0], left + i, 4); memcpy(&words[1], right + i, 4);
            put32(pair, words[0]); put32(pair + 4, words[1]);
            if (fwrite(pair, 1, 8, output) != 8) return AVERROR(EIO);
        }
        *samples += frame->nb_samples;
        av_frame_unref(frame);
    }
    return result == AVERROR(EAGAIN) || result == AVERROR_EOF ? 0 : result;
}

static FILE *create_output(const char *path) {
#ifdef _WIN32
    int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, NULL, 0);
    if (!count) return NULL;
    wchar_t *wide = calloc((size_t)count, sizeof(wchar_t));
    if (!wide) return NULL;
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, wide, count);
    int descriptor = _wopen(wide, _O_CREAT | _O_EXCL | _O_WRONLY | _O_BINARY, _S_IREAD | _S_IWRITE);
    free(wide);
    if (descriptor < 0) return NULL;
    FILE *file = _fdopen(descriptor, "wb");
    if (!file) _close(descriptor);
    return file;
#else
    return fopen(path, "wbx");
#endif
}

static int decode_file(const char *input, const char *destination) {
    AVFormatContext *format = NULL;
    AVCodecContext *decoder = NULL;
    AVPacket *packet = NULL;
    AVFrame *frame = NULL;
    FILE *output = NULL;
    int result, stream, rate = 0;
    uint64_t samples = 0;
    av_log_set_level(AV_LOG_WARNING);
    av_log_set_callback(decoder_log);
    if ((result = avformat_open_input(&format, input, NULL, NULL)) < 0) goto done;
    if ((result = avformat_find_stream_info(format, NULL)) < 0) goto done;
    stream = av_find_best_stream(format, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0);
    if (stream < 0) { result = stream; goto done; }
    if (format->streams[stream]->codecpar->codec_id != AV_CODEC_ID_AC4) {
        result = AVERROR(EINVAL); goto done;
    }
    const AVCodec *codec = avcodec_find_decoder(AV_CODEC_ID_AC4);
    if (!codec) { result = AVERROR_DECODER_NOT_FOUND; goto done; }
    decoder = avcodec_alloc_context3(codec);
    packet = av_packet_alloc(); frame = av_frame_alloc();
    if (!decoder || !packet || !frame) { result = AVERROR(ENOMEM); goto done; }
    if ((result = avcodec_parameters_to_context(decoder, format->streams[stream]->codecpar)) < 0) goto done;
    decoder->err_recognition = AV_EF_EXPLODE | AV_EF_BITSTREAM | AV_EF_BUFFER;
    if ((result = avcodec_open2(decoder, codec, NULL)) < 0) goto done;
    output = create_output(destination);
    if (!output) { result = AVERROR(errno ? errno : EIO); goto done; }
    while ((result = av_read_frame(format, packet)) >= 0) {
        if (packet->stream_index == stream) {
            result = avcodec_send_packet(decoder, packet);
            if (result >= 0) result = drain(decoder, frame, output, &rate, &samples);
        }
        av_packet_unref(packet);
        if (result < 0 || audio_bounds_warning) {
            if (result >= 0) result = AVERROR_INVALIDDATA;
            goto done;
        }
    }
    if (result != AVERROR_EOF) goto done;
    result = avcodec_send_packet(decoder, NULL);
    if (result >= 0) result = drain(decoder, frame, output, &rate, &samples);
    if (result < 0) goto done;
    if (!samples || audio_bounds_warning) { result = AVERROR_INVALIDDATA; goto done; }
    result = wave_header(output, rate, samples);
    if (result >= 0 && fflush(output)) result = AVERROR(EIO);
done:
    if (output && fclose(output) && result >= 0) result = AVERROR(EIO);
    if (result < 0) {
        char error[AV_ERROR_MAX_STRING_SIZE];
        av_strerror(result, error, sizeof(error));
        fprintf(stderr, "AC-4 stereo export failed: %s\n", error);
    } else {
        fprintf(stderr, "Decoded stereo: %d Hz, %llu samples, %.6f seconds; IMS spatial processing not applied\n",
                rate, (unsigned long long)samples, (double)samples / rate);
    }
    av_frame_free(&frame); av_packet_free(&packet);
    avcodec_free_context(&decoder); avformat_close_input(&format);
    return result < 0 ? 1 : 0;
}

#ifdef _WIN32
int wmain(int argc, wchar_t **argv) {
    if (argc != 3) { fprintf(stderr, "Usage: sda-ac4-stereo input.m4a output.wav\n"); return 2; }
    char *paths[2] = {0};
    for (int i = 0; i < 2; i++) {
        int count = WideCharToMultiByte(CP_UTF8, 0, argv[i + 1], -1, NULL, 0, NULL, NULL);
        paths[i] = calloc((size_t)count, 1);
        if (!count || !paths[i]) { free(paths[0]); free(paths[1]); return 1; }
        WideCharToMultiByte(CP_UTF8, 0, argv[i + 1], -1, paths[i], count, NULL, NULL);
    }
    int result = decode_file(paths[0], paths[1]);
    free(paths[0]); free(paths[1]);
    return result;
}
#else
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    return decode_file(argv[1], argv[2]);
}
#endif
