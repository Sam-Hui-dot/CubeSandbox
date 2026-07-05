package com.example.cubesandbox;

import java.net.InetAddress;
import java.net.UnknownHostException;
import java.nio.file.Path;
import java.time.Instant;
import java.util.Map;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/api")
public class InfoController {
    @GetMapping("/info")
    public Map<String, Object> info() {
        return Map.of(
                "app", "cubesandbox-springboot-web",
                "version", "0.1.0",
                "javaVersion", System.getProperty("java.version"),
                "hostname", hostname(),
                "workingDirectory", Path.of("").toAbsolutePath().toString(),
                "timestamp", Instant.now().toString());
    }

    private String hostname() {
        try {
            return InetAddress.getLocalHost().getHostName();
        } catch (UnknownHostException e) {
            return "unknown";
        }
    }
}
