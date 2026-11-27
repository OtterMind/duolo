plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "io.duolo.app"
    compileSdk = 36
    buildToolsVersion = "36.0.0"

    defaultConfig {
        applicationId = "io.duolo.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
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
