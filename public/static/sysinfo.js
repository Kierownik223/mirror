(function () {
    function logError(msg, err) {
        if (window.console && typeof console.error === "function") {
            console.error(msg, err || "");
        } else {
            alert("Error: " + msg + (err ? " - " + err : ""));
        }
    }

    function setText(el, text) {
        if (!el) return;
        if (typeof el.textContent !== "undefined") {
            el.textContent = text;
        } else {
            el.innerText = text;
        }
    }

    function updateInfo() {
        var xhr;
        if (window.XMLHttpRequest) {
            xhr = new XMLHttpRequest();
        } else {
            try {
                xhr = new ActiveXObject("Microsoft.XMLHTTP");
            } catch (e) {
                logError("AJAX not supported", e.message);
                return;
            }
        }

        xhr.open("GET", "/api/sysinfo", true);

        xhr.onreadystatechange = function () {
            if (xhr.readyState == 4) {
                if (xhr.status == 200 || xhr.status == 0) {
                    try {
                        var data = JSON.parse(xhr.responseText);

                        setText(
                            document.getElementById("used_mem_readable"),
                            formatBytes(data.used_mem),
                        );
                        setText(
                            document.getElementById("total_mem_readable"),
                            formatBytes(data.total_mem),
                        );

                        var memUsage = document.getElementById("mem_usage");
                        if (memUsage) {
                            memUsage.max = data.total_mem;
                            memUsage.value = data.used_mem;
                        }

                        var disksContainer = document.getElementById("disks");
                        if (disksContainer) {
                            disksContainer.innerHTML = "";

                            for (var i = 0; i < data.disks.length; i++) {
                                var disk = data.disks[i];
                                var div = document.createElement("div");
                                disksContainer.appendChild(div);
                                try {
                                    div.innerHTML =
                                        '<label for="usage">' +
                                        disk.mount_point +
                                        ": " +
                                        formatBytes(disk.used_space) +
                                        "/" +
                                        formatBytes(disk.total_space) +
                                        '</label><progress style="width:100%; box-sizing:border-box;" class="disk_usage" max="' +
                                        disk.total_space +
                                        '" value="' +
                                        disk.used_space +
                                        '"></progress>';
                                } catch (e) {
                                    div.innerHTML =
                                        disk.mount_point +
                                        ": " +
                                        formatBytes(disk.used_space) +
                                        "/" +
                                        formatBytes(disk.total_space);
                                }
                            }
                        }
                    } catch (e) {
                        logError("JSON parse error", e.message);
                    }
                } else {
                    logError("HTTP error", xhr.status);
                }
            }
        };

        try {
            xhr.send();
        } catch (e) {
            logError("Request send error", e.message);
        }
    }

    function init() {
        setInterval(updateInfo, 2500);
    }

    if (
        document.readyState === "complete" ||
        document.readyState === "interactive"
    ) {
        setTimeout(init, 0);
    } else if (document.addEventListener) {
        document.addEventListener("DOMContentLoaded", init, false);
    } else if (document.attachEvent) {
        document.attachEvent("onreadystatechange", function () {
            if (document.readyState === "complete") {
                init();
            }
        });
    } else {
        window.onload = init;
    }
})();
