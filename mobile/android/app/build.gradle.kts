plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

// 上架签名：发布流程用环境变量传入上传密钥（见 docs/release.md），没有时照常打不签名的包。
val uploadKeystore = System.getenv("DUOLO_UPLOAD_KEYSTORE")?.let(::file)?.takeIf { it.exists() }

android {
    namespace = "io.duolo.app"
    compileSdk = 36
    buildToolsVersion = "36.0.0"

    defaultConfig {
        applicationId = "io.duolo.app"
        minSdk = 26
        targetSdk = 36
        // 发布流程传入：每次上传的 versionCode 要比上次大，versionName 取版本标签。
        versionCode = System.getenv("DUOLO_VERSION_CODE")?.toIntOrNull() ?: 1
        versionName = System.getenv("DUOLO_VERSION_NAME") ?: "0.1.0"
    }

    signingConfigs {
        if (uploadKeystore != null) {
            create("upload") {
                storeFile = uploadKeystore
                storePassword = System.getenv("DUOLO_UPLOAD_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("DUOLO_UPLOAD_KEY_ALIAS")
                keyPassword = System.getenv("DUOLO_UPLOAD_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfigs.findByName("upload")?.let { signingConfig = it }
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    // 语言包只有一份：仓库根目录的 locales，原样打包到 assets/locales；许可文件也是。
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("sharedAssets"))
    sourceSets["main"].res.srcDir(layout.buildDirectory.dir("sharedRes"))

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging { resources.excludes += listOf("META-INF/versions/**", "META-INF/*.kotlin_module") }
}

kotlin { jvmToolchain(17) }

val sharedAssets by tasks.registering(Sync::class) {
    into(layout.buildDirectory.dir("sharedAssets"))
    from(rootDir.resolve("../../locales")) { into("locales") }
    from(rootDir.resolve("../LICENSES.md"), rootDir.resolve("../fonts/OFL.txt"))
}
val sharedRes by tasks.registering(Sync::class) {
    into(layout.buildDirectory.dir("sharedRes"))
    from(rootDir.resolve("../fonts/MonaSans.ttf")) {
        into("font")
        rename { "mona_sans.ttf" }
    }
}
tasks.named("preBuild") { dependsOn(sharedAssets, sharedRes) }

dependencies {
    implementation(project(":core"))
    implementation(libs.coroutines.android)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.runtime)
    implementation(libs.lifecycle.process)
    implementation(libs.navigation.compose)
    implementation(libs.androidx.core)
    implementation(libs.zxing)
}
