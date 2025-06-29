document.addEventListener("DOMContentLoaded", function () {
    var xhr = new XMLHttpRequest();
    xhr.open("GET", "/api/sysinfo", true);
    xhr.onreadystatechange = function () {
        if (xhr.readyState === 4) {
            if (xhr.status === 200) {
                try {
                    var data = JSON.parse(xhr.responseText);
                    var disksContainer = document.getElementById("disks");
                    disksContainer.innerHTML = "";

                    for (var i = 0; i < data.disks.length; i++) {
                        var disk = data.disks[i];

                        var diskDiv = document.createElement("div");
                        diskDiv.innerHTML =
                            '<label for="usage">' + disk.used_space_readable + ' / ' + disk.total_space_readable + '</label>' +
                            '<progress style="width:100%;box-sizing:border-box;" class="disk_usage" max="' + disk.total_space + '" value="' + disk.used_space + '"></progress>';

                        disksContainer.appendChild(diskDiv);
                    }
                } catch (e) {
                    console.error("JSON parse error:", e);
                }
            } else {
                console.error("HTTP error:", xhr.status);
            }
        }
    };
    xhr.onerror = function () {
        console.error("Network error occurred during /api/sysinfo fetch.");
    };
    xhr.send();
});