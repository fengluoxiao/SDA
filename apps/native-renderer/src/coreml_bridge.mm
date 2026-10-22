// Original audio spectral-MAC bridge using public Core ML APIs only.
// All functions run on the optional accelerator worker, never the audio callback.
#import <Foundation/Foundation.h>
#import <CoreML/CoreML.h>
#include "coreml_bridge.h"
#include <memory>
#include <cstdio>
#include <cmath>

static void failure(char *out, size_t cap, NSString *message) {
    if (out && cap) snprintf(out,cap,"%s",(message ?: @"Core ML error").UTF8String);
}
struct AudioModel {
    __strong MLModel *model = nil;
    __strong NSURL *root = nil;
    size_t rows=0,bins=0,inputs=0;
    ~AudioModel() {
        model=nil;
        if (root) [[NSFileManager defaultManager] removeItemAtURL:root error:nil];
    }
};
int sda_coreml_neural_engine_available(void) {
    @autoreleasepool {
        @try {
            if (@available(iOS 17.0, macOS 14.0, *)) {
                for (id<MLComputeDeviceProtocol> device in MLModel.availableComputeDevices) {
                    if ([(NSObject *)device isKindOfClass:MLNeuralEngineComputeDevice.class]) return 1;
                }
                return 0;
            }
            return -1;
        } @catch (NSException *e) { return -1; }
    }
}
static bool valid_feature(MLFeatureDescription *f,size_t rows,size_t bins) {
    MLMultiArrayConstraint *a=f.multiArrayConstraint;
    return f.type==MLFeatureTypeMultiArray && a.dataType==MLMultiArrayDataTypeFloat32 &&
        a.shape.count==2 && a.shape[0].unsignedIntegerValue==rows && a.shape[1].unsignedIntegerValue==bins;
}
void *sda_coreml_create(const uint8_t *bytes,size_t length,size_t rows,size_t bins,
                       size_t inputs,int cpu_only,char *error,size_t cap) {
    @autoreleasepool { @try {
        if (!bytes || !length || length>1024*1024 || rows<1 || rows>256 || bins<2 || bins>4097 ||
            inputs<6 || inputs>24 || inputs%6) {failure(error,cap,@"invalid-model-contract");return nullptr;}
        if (@available(iOS 16.0,macOS 13.0,*)) {
            auto ctx=std::make_unique<AudioModel>();ctx->rows=rows;ctx->bins=bins;ctx->inputs=inputs;
            NSFileManager *fm=NSFileManager.defaultManager;
            NSURL *cache=[[fm URLsForDirectory:NSCachesDirectory inDomains:NSUserDomainMask] firstObject];
            if (!cache) {failure(error,cap,@"model-cache-unavailable");return nullptr;}
            // One private graph slot, containing no PCM/HRIR/input weights.
            ctx->root=[cache URLByAppendingPathComponent:@"sda-coreml-spectral-v1" isDirectory:YES];
            [fm removeItemAtURL:ctx->root error:nil];
            NSError *e=nil;
            if (![fm createDirectoryAtURL:ctx->root withIntermediateDirectories:YES attributes:nil error:&e]) {
                failure(error,cap,e.localizedDescription);return nullptr;
            }
            NSURL *source=[ctx->root URLByAppendingPathComponent:@"spectral.mlmodel"];
            if (![[NSData dataWithBytes:bytes length:length] writeToURL:source options:NSDataWritingAtomic error:&e]) {
                failure(error,cap,e.localizedDescription);return nullptr;
            }
            NSURL *temporary=[MLModel compileModelAtURL:source error:&e];
            if (!temporary) {failure(error,cap,e.localizedDescription);return nullptr;}
            NSURL *compiled=[ctx->root URLByAppendingPathComponent:@"spectral.mlmodelc" isDirectory:YES];
            BOOL copied=[fm copyItemAtURL:temporary toURL:compiled error:&e];
            [fm removeItemAtURL:temporary error:nil];
            if (!copied) {failure(error,cap,e.localizedDescription);return nullptr;}
            MLModelConfiguration *configuration=[MLModelConfiguration new];
            configuration.computeUnits=cpu_only ? MLComputeUnitsCPUOnly : MLComputeUnitsCPUAndNeuralEngine;
            ctx->model=[MLModel modelWithContentsOfURL:compiled configuration:configuration error:&e];
            if (!ctx->model) {failure(error,cap,e.localizedDescription);return nullptr;}
            MLModelDescription *description=ctx->model.modelDescription;
            if (description.inputDescriptionsByName.count!=inputs || description.outputDescriptionsByName.count!=4) {
                failure(error,cap,@"compiled-feature-count-mismatch");return nullptr;
            }
            for (size_t i=0;i<inputs;i++) {
                if (!valid_feature(description.inputDescriptionsByName[[NSString stringWithFormat:@"v%zu",i]],rows,bins)) {
                    failure(error,cap,@"compiled-input-contract-mismatch");return nullptr;
                }
            }
            for (size_t i=0;i<4;i++) {
                if (!valid_feature(description.outputDescriptionsByName[[NSString stringWithFormat:@"o%zu",i]],rows,bins)) {
                    failure(error,cap,@"compiled-output-contract-mismatch");return nullptr;
                }
            }
            return ctx.release();
        }
        failure(error,cap,@"Core-ML-CPU-and-Neural-Engine-requires-iOS-16");return nullptr;
    } @catch (NSException *e) {failure(error,cap,e.reason);return nullptr;} }
}
int sda_coreml_predict(void *handle,const float *input,size_t input_length,
                      float *output,size_t output_length,char *error,size_t cap) {
    @autoreleasepool { @try {
        AudioModel *ctx=static_cast<AudioModel *>(handle);
        if (!ctx || !input || !output || input_length!=ctx->rows*ctx->bins*ctx->inputs || output_length!=ctx->rows*ctx->bins*4) {
            failure(error,cap,@"prediction-buffer-contract-mismatch");return -1;
        }
        if (@available(iOS 16.0,macOS 13.0,*)) {
            const size_t plane=ctx->rows*ctx->bins;
            NSArray<NSNumber *> *shape=@[@(ctx->rows),@(ctx->bins)];
            NSArray<NSNumber *> *strides=@[@(ctx->bins),@1];
            NSMutableDictionary<NSString *,MLFeatureValue *> *values=[NSMutableDictionary new];
            NSMutableDictionary<NSString *,MLMultiArray *> *backings=[NSMutableDictionary new];
            NSError *e=nil;
            for (size_t i=0;i<ctx->inputs;i++) {
                MLMultiArray *array=[[MLMultiArray alloc] initWithDataPointer:const_cast<float *>(input+i*plane)
                    shape:shape dataType:MLMultiArrayDataTypeFloat32 strides:strides deallocator:^(void *){} error:&e];
                if (!array) {failure(error,cap,e.localizedDescription);return -2;}
                values[[NSString stringWithFormat:@"v%zu",i]]=[MLFeatureValue featureValueWithMultiArray:array];
            }
            for (size_t i=0;i<4;i++) {
                MLMultiArray *array=[[MLMultiArray alloc] initWithDataPointer:output+i*plane
                    shape:shape dataType:MLMultiArrayDataTypeFloat32 strides:strides deallocator:^(void *){} error:&e];
                if (!array) {failure(error,cap,e.localizedDescription);return -2;}
                backings[[NSString stringWithFormat:@"o%zu",i]]=array;
            }
            MLDictionaryFeatureProvider *features=[[MLDictionaryFeatureProvider alloc] initWithDictionary:values error:&e];
            if (!features) {failure(error,cap,e.localizedDescription);return -2;}
            MLPredictionOptions *options=[MLPredictionOptions new];options.outputBackings=backings;
            id<MLFeatureProvider> result=[ctx->model predictionFromFeatures:features options:options error:&e];
            if (!result) {failure(error,cap,e.localizedDescription);return -3;}
            for (size_t i=0;i<4;i++) {
                MLMultiArray *array=[result featureValueForName:[NSString stringWithFormat:@"o%zu",i]].multiArrayValue;
                if (array.dataType!=MLMultiArrayDataTypeFloat32 || array.shape.count!=2 ||
                    array.shape[0].unsignedIntegerValue!=ctx->rows || array.shape[1].unsignedIntegerValue!=ctx->bins) {
                    failure(error,cap,@"prediction-output-contract-mismatch");return -4;
                }
                if (array.dataPointer==output+i*plane) continue;
                const float *data=static_cast<const float *>(array.dataPointer);
                const size_t rowStride=array.strides[0].unsignedIntegerValue, binStride=array.strides[1].unsignedIntegerValue;
                for (size_t row=0;row<ctx->rows;row++) for(size_t bin=0;bin<ctx->bins;bin++) {
                    output[i*plane+row*ctx->bins+bin]=data[row*rowStride+bin*binStride];
                }
            }
            return 0;
        }
        failure(error,cap,@"unsupported-Core-ML-OS");return -5;
    } @catch (NSException *e) {failure(error,cap,e.reason);return -6;} }
}
void sda_coreml_free(void *handle) {
    @autoreleasepool { @try {delete static_cast<AudioModel *>(handle);} @catch(NSException *){} }
}
