plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.android.library)
}

kotlin {
    androidTarget()
    jvmToolchain(21)

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
        val androidUnitTest by getting {
            dependencies {
                implementation(kotlin("test"))
            }
        }
    }
}

android {
    namespace = "ws.klimek.gamiscreen.pwashell"
    compileSdk = 36

    defaultConfig {
        minSdk = 31
    }

    sourceSets["main"].manifest.srcFile("src/androidMain/AndroidManifest.xml")

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
}
