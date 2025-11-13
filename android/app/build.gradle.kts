import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.io.File

fun readWorkspaceVersion(cargoFile: File?): String {
    if (cargoFile == null || !cargoFile.exists()) {
        return "0.0.0"
    }
    var inWorkspacePackage = false
    cargoFile.readLines().forEach { line ->
        val trimmed = line.trim()
        if (trimmed.startsWith("[")) {
            inWorkspacePackage = trimmed == "[workspace.package]"
        } else if (inWorkspacePackage && trimmed.startsWith("version")) {
            val value = trimmed.substringAfter("=").trim().trim('"')
            if (value.isNotEmpty()) {
                return value
            }
        }
    }
    return "0.0.0"
}

fun versionCodeFrom(version: String): Int {
    val parts = version.split(".")
    val major = parts.getOrNull(0)?.toIntOrNull() ?: 0
    val minor = parts.getOrNull(1)?.toIntOrNull() ?: 0
    val patch = parts.getOrNull(2)?.toIntOrNull() ?: 0
    return major * 10000 + minor * 100 + patch
}

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.compose.multiplatform)
}

val embeddedPwaDir = layout.projectDirectory.dir("../../gamiscreen-web/dist")
val workspaceRoot = rootProject.projectDir.parentFile
val workspaceVersion = readWorkspaceVersion(workspaceRoot?.resolve("Cargo.toml"))
val workspaceVersionCode = versionCodeFrom(workspaceVersion)

android {
    namespace = "ws.klimek.gamiscreen.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "ws.klimek.gamiscreen.app"
        minSdk = 31
        targetSdk = 34
        versionCode = workspaceVersionCode
        versionName = workspaceVersion

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
    val variantNameCap = variant.name.replaceFirstChar { it.uppercaseChar() }
    val taskProvider = tasks.register("verify${variantNameCap}EmbeddedAssets") {
        inputs.dir(embeddedDir)
        doFirst {
            if (!embeddedDir.exists()) {
                logger.warn(
                    "Embedded PWA assets not found at ${embeddedDir.absolutePath}. " +
                        "Continuing with remote PWA."
                )
            }
        }
    }
    tasks.configureEach {
        if (name == "assemble${variantNameCap}" ||
            name == "bundle${variantNameCap}" ||
            name == "install${variantNameCap}" ||
            name == "package${variantNameCap}" ||
            name == "lint${variantNameCap}" ||
            name == "connected${variantNameCap}AndroidTest"
        ) {
            dependsOn(taskProvider)
        }
    }
}
