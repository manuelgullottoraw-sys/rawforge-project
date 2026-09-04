plugins {
    kotlin("multiplatform")
    id("com.android.library")
    id("org.jetbrains.compose")
}

kotlin {
    androidTarget()
    jvm("desktop")

    sourceSets {
        val commonMain by getting {
            dependencies {
                api(compose.runtime)
                api(compose.foundation)
                api(compose.material)
                api(compose.ui)
            }
        }
        val androidMain by getting {
            dependencies {
                // Il motore Rust è caricato via JNA dai binding generati da UniFFI
                // (docs/ARCHITECTURE.md, §1/§7). Variante @aar: build JNA compatibile Android.
                implementation("net.java.dev.jna:jna:5.14.0@aar")
            }
        }
        val desktopMain by getting {
            dependencies {
                api(compose.desktop.common)
                implementation("net.java.dev.jna:jna:5.14.0")
            }
        }
    }
}

android {
    namespace = "com.rawforge.shared"
    compileSdk = 34

    defaultConfig {
        minSdk = 24
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
