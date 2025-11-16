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

/**
 * Builds an integer version code using 4 segments: MMM.NNN.PPP.QQQ
 *
 *  - MMM: major (0–999)
 *  - NNN: minor (0–999)
 *  - PPP: patch (0–999)
 *  - QQQ: qualifier (0–999) from the trailing number in the qualifier suffix.
 *         If no numeric qualifier is present, defaults to 999 so releases stay highest.
 */
fun versionCodeFrom(version: String): Int {
    fun parseSegment(value: String?): Int = value?.toIntOrNull()?.coerceIn(0, 999) ?: 0

    val (semantic, qualifierSuffix) = version.split("-", limit = 2).let {
        it[0] to it.getOrNull(1)
    }
    val semanticParts = semantic.split(".")

    val major = parseSegment(semanticParts.getOrNull(0))
    val minor = parseSegment(semanticParts.getOrNull(1))
    val patch = parseSegment(semanticParts.getOrNull(2))

    val qualifier = qualifierSuffix
        ?.let { Regex("(\\d+)$").find(it)?.value?.toIntOrNull() }
        ?.coerceIn(0, 999)
        ?: 999

    return major * 1_000_000_000 +
        minor * 1_000_000 +
        patch * 1_000 +
        qualifier
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
val signingKeystorePath = System.getenv("ANDROID_SIGNING_KEYSTORE")?.takeIf { it.isNotBlank() }
val signingKeystorePassword = System.getenv("ANDROID_SIGNING_KEYSTORE_PASSWORD")
val signingKeyAlias = System.getenv("ANDROID_SIGNING_KEY_ALIAS")
val signingKeyAliasPassword = System.getenv("ANDROID_SIGNING_KEY_ALIAS_PASSWORD")

android {
    namespace = "ws.klimek.gamiscreen.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "ws.klimek.gamiscreen.app"
        minSdk = 31
        targetSdk = 36
        versionCode = workspaceVersionCode
        versionName = workspaceVersion

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        if (signingKeystorePath != null &&
            signingKeystorePassword != null &&
            signingKeyAlias != null &&
            signingKeyAliasPassword != null
        ) {
            create("release") {
                storeFile = file(signingKeystorePath)
                storePassword = signingKeystorePassword
                keyAlias = signingKeyAlias
                keyPassword = signingKeyAliasPassword
            }
        }
    }

    buildTypes {
        getByName("debug") {
            buildConfigField("boolean", "EMBED_PWA", "true")
        }
        getByName("release") {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            buildConfigField("boolean", "EMBED_PWA", "true")
            ndk {
                debugSymbolLevel = "FULL"
            }
            signingConfigs.findByName("release")?.let {
                signingConfig = it
            }
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
    sourceSets["release"].assets.srcDir(embeddedPwaDir)

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
    implementation(libs.jsr305)

    debugImplementation(compose.uiTooling)
}

androidComponents.onVariants { variant ->
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
