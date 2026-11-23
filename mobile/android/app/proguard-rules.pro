# kotlinx.serialization 自带规则。
# OkHttp 用到的、Android 上没有的类。
-dontwarn org.bouncycastle.jsse.**
-dontwarn org.conscrypt.**
-dontwarn org.openjsse.**
# BouncyCastle 只用到轻量接口，里面引用的 JNDI 在 Android 上没有。
-dontwarn javax.naming.**
