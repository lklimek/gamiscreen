plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.android.kmp.library)
}

kotlin {
    android {
        namespace = "ws.klimek.gamiscreen.pwashell"
        compileSdk {
            version = release(37) { minorApiLevel = 2 }
        }
        minSdk = 31
        androidResources.enable = true
        withHostTest {}
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21)
        }
    }
    jvmToolchain(25)

    sourceSets {
        val commonMain by getting {
            dependencies {
                implementation(compose.runtime)
                implementation(compose.foundation)
                implementation(compose.material3)
                implementation(compose.ui)
            }
        }
        val androidMain by getting {
            dependencies {
                implementation(projects.core)
                implementation(libs.androidx.activity.compose)
                implementation(libs.androidx.lifecycle.runtime)
                implementation(libs.androidx.webkit)
                implementation(libs.kotlinx.coroutines.android)
                implementation(compose.preview)
            }
        }
        val androidHostTest by getting {
            dependencies {
                implementation(kotlin("test"))
            }
        }
    }
}
