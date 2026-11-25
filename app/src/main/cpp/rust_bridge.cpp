#include <jni.h>
#include <cstddef>
#include <cstdint>

extern "C" uint32_t nekoflash_adb_checksum(const uint8_t* data, size_t length);

extern "C" JNIEXPORT jint JNICALL
Java_ru_forum_adbfastboottool_RustCore_checksum(JNIEnv* env, jobject, jbyteArray payload) {
    const auto length = env->GetArrayLength(payload);
    if (length == 0) return 0;
    // No JNI calls or blocking I/O while the array is borrowed.
    auto* data = static_cast<jbyte*>(env->GetPrimitiveArrayCritical(payload, nullptr));
    if (data == nullptr) return 0;  // VM has a pending allocation exception.
    const auto sum = nekoflash_adb_checksum(reinterpret_cast<const uint8_t*>(data), length);
    env->ReleasePrimitiveArrayCritical(payload, data, JNI_ABORT);
    return static_cast<jint>(sum);
}

struct SparseInfo { uint32_t block_size, blocks, chunks; };
struct CheckCancelled { JNIEnv* env; jobject check; jmethodID run; };
extern "C" int32_t nekoflash_sparse_info(int32_t, void*, bool (*)(void*), SparseInfo*);

extern "C" JNIEXPORT jlongArray JNICALL
Java_ru_forum_adbfastboottool_RustCore_sparseInfo(JNIEnv* env, jobject, jint fd, jobject check) {
    const auto cls = env->GetObjectClass(check);
    if (!cls) return nullptr;
    CheckCancelled callback{env, check, env->GetMethodID(cls, "run", "()V")};
    if (!callback.run) return nullptr;
    SparseInfo info{};
    const auto status = nekoflash_sparse_info(fd, &callback, [](void* context) {
        auto* c = static_cast<CheckCancelled*>(context);
        c->env->CallVoidMethod(c->check, c->run);
        return c->env->ExceptionCheck() == JNI_TRUE;
    }, &info);
    if (env->ExceptionCheck()) return nullptr;
    if (status != 0) {
        auto type = env->FindClass(status == 1 ? "java/lang/IllegalArgumentException" : "java/io/IOException");
        if (type) env->ThrowNew(type, status == 1 ? "Invalid Android sparse image" : "Cannot read sparse image");
        return nullptr;
    }
    const jlong values[] = {info.block_size, info.blocks, info.chunks};
    auto result = env->NewLongArray(3);
    if (result) env->SetLongArrayRegion(result, 0, 3, values);
    return result;
}
