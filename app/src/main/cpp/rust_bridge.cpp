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
