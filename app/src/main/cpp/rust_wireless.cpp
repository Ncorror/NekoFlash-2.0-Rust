#include <jni.h>
#include <cstddef>
#include <cstdint>
#include <string>

struct WirelessCallbacks {
    void* context;
    bool (*read)(void*, uint8_t*, size_t);
    bool (*write)(void*, const uint8_t*, size_t);
    bool (*upgrade)(void*);
    void (*connected)(void*, uint32_t, uint32_t, const uint8_t*, size_t);
    void (*failed)(void*, const uint8_t*, size_t);
};
extern "C" void nekoflash_wireless_handshake(const WirelessCallbacks*);

namespace {
struct Handshake {
    JNIEnv* env;
    jobject read, write, upgrade;
    jintArray parameters;
    jmethodID accept, run;
    jbyteArray banner;
};

bool readFully(void* context, uint8_t* bytes, size_t size) {
    auto& h = *static_cast<Handshake*>(context);
    auto length = static_cast<jsize>(size);
    auto buf = h.env->NewByteArray(length);
    if (!buf) return false;
    h.env->CallVoidMethod(h.read, h.accept, buf);
    if (!h.env->ExceptionCheck())
        h.env->GetByteArrayRegion(buf, 0, length, reinterpret_cast<jbyte*>(bytes));
    h.env->DeleteLocalRef(buf);
    return !h.env->ExceptionCheck();
}

jbyteArray copyBytes(JNIEnv* env, const uint8_t* bytes, size_t size) {
    auto length = static_cast<jsize>(size);
    auto buf = env->NewByteArray(length);
    if (buf && length) env->SetByteArrayRegion(buf, 0, length, reinterpret_cast<const jbyte*>(bytes));
    return buf;
}

bool writeFully(void* context, const uint8_t* bytes, size_t size) {
    auto& h = *static_cast<Handshake*>(context);
    auto buf = copyBytes(h.env, bytes, size);
    if (!buf) return false;
    if (!h.env->ExceptionCheck()) h.env->CallVoidMethod(h.write, h.accept, buf);
    h.env->DeleteLocalRef(buf);
    return !h.env->ExceptionCheck();
}

bool startTls(void* context) {
    auto& h = *static_cast<Handshake*>(context);
    h.env->CallVoidMethod(h.upgrade, h.run);
    return !h.env->ExceptionCheck();
}

void connected(void* context, uint32_t version, uint32_t max, const uint8_t* bytes, size_t size) {
    auto& h = *static_cast<Handshake*>(context);
    const jint values[] = {static_cast<jint>(version), static_cast<jint>(max)};
    h.env->SetIntArrayRegion(h.parameters, 0, 2, values);
    if (!h.env->ExceptionCheck()) h.banner = copyBytes(h.env, bytes, size);
}

void failed(void* context, const uint8_t* message, size_t size) {
    auto& h = *static_cast<Handshake*>(context);
    if (h.env->ExceptionCheck()) return; // Keep the socket/TLS exception from Android.
    auto cls = h.env->FindClass("java/io/IOException");
    if (cls) h.env->ThrowNew(cls, std::string(reinterpret_cast<const char*>(message), size).c_str());
}
} // namespace

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ru_forum_adbfastboottool_RustCore_wirelessHandshake(JNIEnv* env, jobject, jobject read,
        jobject write, jobject upgrade, jintArray parameters) {
    auto consumer = env->FindClass("java/util/function/Consumer");
    if (!consumer) return nullptr;
    auto accept = env->GetMethodID(consumer, "accept", "(Ljava/lang/Object;)V");
    if (!accept) return nullptr;
    auto runnable = env->FindClass("java/lang/Runnable");
    if (!runnable) return nullptr;
    auto run = env->GetMethodID(runnable, "run", "()V");
    if (!run) return nullptr;
    Handshake h{env, read, write, upgrade, parameters, accept, run, nullptr};
    WirelessCallbacks io{&h, readFully, writeFully, startTls, connected, failed};
    nekoflash_wireless_handshake(&io);
    return h.banner;
}
