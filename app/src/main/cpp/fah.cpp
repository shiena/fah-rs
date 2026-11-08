#include "pipeline.h"
#include <android/bitmap.h>
#include <jni.h>
#include <memory>
#include <mutex>
#include <stdexcept>

namespace {
std::unique_ptr<fah::Pipeline> pipeline;
std::mutex mutex;
void error(JNIEnv* env,const std::exception& e) { env->ThrowNew(env->FindClass("java/lang/IllegalStateException"),e.what()); }
}
extern "C" JNIEXPORT jstring JNICALL Java_dev_fah_android_MainActivity_nativeInit(JNIEnv* env,jobject,jstring path,jboolean gpu) {
    std::lock_guard<std::mutex> lock(mutex);
    const char* chars=env->GetStringUTFChars(path,nullptr);
    std::string directory(chars);
    env->ReleaseStringUTFChars(path,chars);
    try {
        pipeline=std::make_unique<fah::Pipeline>(directory,gpu);
        return env->NewStringUTF(fah::gpuInfo().c_str());
    } catch(const std::exception& e) { error(env,e);return nullptr; }
}
extern "C" JNIEXPORT jstring JNICALL Java_dev_fah_android_MainActivity_nativeProcess(JNIEnv* env,jobject,jobject bitmap,jboolean reset) {
    std::lock_guard<std::mutex> lock(mutex);
    void* pixels=nullptr;
    try {
        if(!pipeline) throw std::runtime_error("Models are not loaded");
        AndroidBitmapInfo info{};
        if(AndroidBitmap_getInfo(env,bitmap,&info)!=0||info.format!=ANDROID_BITMAP_FORMAT_RGBA_8888)
            throw std::runtime_error("Expected RGBA bitmap");
        if(AndroidBitmap_lockPixels(env,bitmap,&pixels)!=0) throw std::runtime_error("Cannot lock bitmap");
        auto result=pipeline->process(static_cast<uint8_t*>(pixels),info.width,info.height,info.stride,reset);
        AndroidBitmap_unlockPixels(env,bitmap);pixels=nullptr;
        return env->NewStringUTF(result.c_str());
    } catch(const std::exception& e) {
        if(pixels) AndroidBitmap_unlockPixels(env,bitmap);
        error(env,e);return nullptr;
    }
}
extern "C" JNIEXPORT void JNICALL Java_dev_fah_android_MainActivity_nativeClose(JNIEnv*,jobject) {
    std::lock_guard<std::mutex> lock(mutex);pipeline.reset();
}
