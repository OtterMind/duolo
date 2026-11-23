plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.kotlin.serialization)
}

kotlin { jvmToolchain(17) }

dependencies {
    api(libs.coroutines.core)
    api(libs.serialization.json)
    api(libs.okhttp)
    implementation(libs.bouncycastle)
    testImplementation(libs.junit)
    testImplementation(libs.sqlite.jdbc)
}

tasks.test {
    // 三端共用的样例和语言包在仓库里。
    systemProperty("duolo.repo", rootDir.resolve("../..").canonicalPath)
    inputs.dir(rootDir.resolve("../contract"))
    inputs.dir(rootDir.resolve("../../locales"))
    // 和联调沙箱真的走一遍：DUOLO_SANDBOX=http://localhost:8788 ./gradlew :core:test
    System.getenv("DUOLO_SANDBOX")?.let {
        environment("DUOLO_SANDBOX", it)
        outputs.upToDateWhen { false }
    }
    testLogging { events("failed"); exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL }
}
