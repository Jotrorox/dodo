import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.kotlin.gradle.dsl.JvmDefaultMode
import org.jetbrains.intellij.platform.gradle.TestFrameworkType

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.4.21"
    id("org.jetbrains.intellij.platform") version "2.19.0"
}

group = "io.github.jotrorox"
version = providers.gradleProperty("pluginVersion").get()

kotlin {
    // The 2026.2 platform is compiled for Java 25 and runs on JetBrains Runtime 25.
    jvmToolchain(25)
    compilerOptions {
        // Call the platform's interface defaults directly instead of generating bridges to them.
        jvmDefault = JvmDefaultMode.NO_COMPATIBILITY
    }
}

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.opentest4j:opentest4j:1.3.0")

    intellijPlatform {
        // -PlocalIdePath builds and tests against an installed IDE, such as RustRover;
        // otherwise the CLion release is downloaded.
        val localIde = providers.gradleProperty("localIdePath").orNull
        if (localIde != null) local(localIde) else clion(providers.gradleProperty("platformVersion"))
        testFramework(TestFrameworkType.Platform)
    }
}

/**
 * Converts the VS Code extension's snippets into a live template set, so both
 * editors offer the same snippets from one source. `${1:name}` placeholders become
 * template variables in tab order, and `$0` becomes the final caret position.
 */
abstract class GenerateLiveTemplates : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val snippets: RegularFileProperty

    @get:OutputDirectory
    abstract val outputDirectory: DirectoryProperty

    @TaskAction
    fun generate() {
        @Suppress("UNCHECKED_CAST")
        val json = groovy.json.JsonSlurper().parse(snippets.get().asFile) as Map<String, Map<String, Any>>
        // A placeholder, or any other dollar sign (VS Code escapes it as `\$`).
        val placeholder = Regex("""\$\{(\d+)(?::([^}]*))?}|\$(\d+)|\\?\$""")
        fun attr(value: String) = value
            .replace("&", "&amp;").replace("\"", "&quot;").replace("<", "&lt;").replace(">", "&gt;")
            .replace("\n", "&#10;")
        val xml = StringBuilder("<templateSet group=\"Dodo\">\n")
        for ((_, snippet) in json) {
            val prefix = snippet.getValue("prefix") as String
            val body = when (val value = snippet.getValue("body")) {
                is List<*> -> value.joinToString("\n")
                else -> value as String
            }
            val defaults = sortedMapOf<Int, String>()
            val text = placeholder.replace(body) { match ->
                val index = (match.groups[1] ?: match.groups[3])?.value?.toInt()
                if (index == null) {
                    "$$"
                } else if (index == 0) {
                    "\$END\$"
                } else {
                    val default = match.groups[2]?.value
                    if (default != null || index !in defaults) defaults[index] = default ?: defaults[index] ?: ""
                    "\$P$index\$"
                }
            }
            xml.append("  <template name=\"${attr(prefix)}\" value=\"${attr(text)}\" ")
                .append("description=\"${attr(snippet["description"] as String? ?: prefix)}\" ")
                .append("toReformat=\"false\" toShortenFQNames=\"false\">\n")
            for ((index, default) in defaults) {
                val quoted = "\"" + default.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
                xml.append("    <variable name=\"P$index\" expression=\"\" defaultValue=\"${attr(quoted)}\" alwaysStopAt=\"true\"/>\n")
            }
            xml.append("    <context><option name=\"DODO\" value=\"true\"/></context>\n  </template>\n")
        }
        xml.append("</templateSet>\n")
        val output = outputDirectory.get().file("liveTemplates/Dodo.xml").asFile
        output.parentFile.mkdirs()
        output.writeText(xml.toString())
    }
}

val generateLiveTemplates = tasks.register<GenerateLiveTemplates>("generateLiveTemplates") {
    snippets = layout.projectDirectory.file("../dodo-vscode/snippets/dodo.json")
    outputDirectory = layout.buildDirectory.dir("generated/liveTemplates")
}

sourceSets.main {
    resources.srcDir(generateLiveTemplates)
}

intellijPlatform {
    pluginConfiguration {
        version = providers.gradleProperty("pluginVersion")
        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            untilBuild = provider { null }
        }
    }
    pluginVerification {
        ides {
            // Installed IDEs (comma-separated paths) avoid downloading the releases.
            val localIdes = providers.gradleProperty("verifyIdePaths").orNull
            if (localIdes != null) {
                localIdes.split(',').map(String::trim).filter(String::isNotEmpty).forEach { local(it) }
            } else {
                create(IntelliJPlatformType.CLion, providers.gradleProperty("platformVersion"))
                create(IntelliJPlatformType.RustRover, providers.gradleProperty("platformVersion"))
            }
        }
    }
}
