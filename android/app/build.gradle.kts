import org.gradle.api.GradleException
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.compose.multiplatform)
}

val embeddedPwaDir = layout.projectDirectory.dir("../gamiscreen-web/dist")

android {
    namespace = "ws.klimek.gamiscreen.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "ws.klimek.gamiscreen.app"
        minSdk = 31
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        getByName("debug") {
            buildConfigField("boolean", "EMBED_PWA", "true")
        }
        getByName("release") {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            buildConfigField("boolean", "EMBED_PWA", "false")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["debug"].assets.srcDir(embeddedPwaDir)

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_21)
    }
    jvmToolchain(21)
}

dependencies {
    implementation(projects.core)
    implementation(projects.pwaShell)

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime)
    implementation(libs.androidx.activity.compose)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.material)
    implementation(compose.runtime)
    implementation(compose.foundation)
    implementation(compose.material3)
    implementation(compose.ui)
    implementation(compose.preview)

    debugImplementation(compose.uiTooling)
}

androidComponents.onVariants { variant ->
    if (variant.buildType != "debug") return@onVariants
    val embeddedDir = embeddedPwaDir.asFile
    tasks.register("verify${variant.name.replaceFirstChar { it.uppercaseChar() }}EmbeddedAssets") {
        inputs.dir(embeddedDir)
        doFirst {
            if (!embeddedDir.exists()) {
                throw GradleException(
                    "Embedded PWA assets not found. Run `npm run build` inside gamiscreen-web/ before building the debug app."
                )
            }
        }
    }
}
