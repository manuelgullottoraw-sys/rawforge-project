import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    kotlin("jvm")
    id("org.jetbrains.compose")
}

dependencies {
    implementation(project(":shared"))
    implementation(compose.desktop.currentOs)
}

compose.desktop {
    application {
        mainClass = "MainKt"

        nativeDistributions {
            targetFormats(TargetFormat.Exe)
            packageName = "RawForge"
            packageVersion = "0.1.0"
            description = "RawForge — motore RAW ultra-veloce"

            windows {
                menuGroup = "RawForge"
                upgradeUuid = "5b1f6a2e-9c3d-4a11-8b7a-3f6c9e2d7a10"
            }
        }
    }
}
