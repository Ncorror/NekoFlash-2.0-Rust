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

using EmitToken = bool (*)(void*, const uint16_t*, size_t, const uint16_t*, size_t);
extern "C" bool nekoflash_tokenize(const uint16_t*, size_t, void*, EmitToken);

extern "C" JNIEXPORT void JNICALL
Java_ru_forum_adbfastboottool_RustCore_tokenize(JNIEnv* env, jobject, jstring text, jobject sink) {
    const auto cls = env->GetObjectClass(sink);
    if (!cls) return;
    const auto accept = env->GetMethodID(cls, "accept", "(Ljava/lang/Object;Ljava/lang/Object;)V");
    if (!accept) return;
    struct Sink { JNIEnv* env; jobject obj; jmethodID accept; } callback{env, sink, accept};
    const auto length = env->GetStringLength(text);
    const auto* chars = env->GetStringChars(text, nullptr);
    if (!chars) return;
    const bool valid = nekoflash_tokenize(chars, length, &callback,
        [](void* ptr, const uint16_t* value, size_t n, const uint16_t* literal, size_t size) {
            auto* c = static_cast<Sink*>(ptr);
            const auto a = c->env->NewString(value, static_cast<jsize>(n));
            if (!a) return false;
            const auto b = c->env->NewString(literal, static_cast<jsize>(size));
            if (b) {
                c->env->CallVoidMethod(c->obj, c->accept, a, b);
                c->env->DeleteLocalRef(b);
            }
            c->env->DeleteLocalRef(a);
            return c->env->ExceptionCheck() == JNI_FALSE;
        });
    env->ReleaseStringChars(text, chars);
    if (!valid && !env->ExceptionCheck()) {
        auto type = env->FindClass("java/lang/IllegalArgumentException");
        if (type) env->ThrowNew(type, "Unclosed quote or trailing escape in Fastboot command");
    }
}

struct VariableReply { uint32_t length; uint8_t bytes[4096]; };
extern "C" int32_t nekoflash_getvar(int32_t, int32_t, int32_t, const char*, const char*, VariableReply*);

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ru_forum_adbfastboottool_RustCore_queryVariable(JNIEnv* env, jobject, jint fd, jint ep_in,
                                                     jint ep_out, jstring peer, jstring name) {
    const auto* var = env->GetStringUTFChars(name, nullptr);
    if (!var) return nullptr;
    const char* address = peer ? env->GetStringUTFChars(peer, nullptr) : nullptr;
    if (peer && !address) {
        env->ReleaseStringUTFChars(name, var);
        return nullptr;
    }
    VariableReply reply{};
    const auto status = static_cast<jbyte>(nekoflash_getvar(fd, ep_in, ep_out, address, var, &reply));
    if (peer) env->ReleaseStringUTFChars(peer, address);
    env->ReleaseStringUTFChars(name, var);
    const auto length = static_cast<jsize>(reply.length);
    auto packet = env->NewByteArray(length + 1);
    if (!packet) return nullptr;
    env->SetByteArrayRegion(packet, 0, 1, &status);
    env->SetByteArrayRegion(packet, 1, length, reinterpret_cast<const jbyte*>(reply.bytes));
    return packet;
}
