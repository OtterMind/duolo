pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "Duolo"

// core：协议、加密、同步、本地保存、列表和时间线的推导，纯 Kotlin，不依赖 Android，单元测试就能测。
// app：界面和系统能力。
include(":core", ":app")
