document.addEventListener("DOMContentLoaded", function () {
    function updateInfo() {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", "/api/sysinfo", true);
        xhr.onreadystatechange = function () {
            if (xhr.readyState === 4) {
                if (xhr.status === 200) {
                    try {
                        var data = JSON.parse(xhr.responseText);

                        document.getElementById("used_mem_readable").textContent = data.used_mem_readable;
                        document.getElementById("total_mem_readable").textContent = data.total_mem_readable;

                        var memUsage = document.getElementById("mem_usage");
                        memUsage.max = data.total_mem;
                        memUsage.value = data.used_mem;

                        var disksContainer = document.getElementById("disks");
                        disksContainer.innerHTML = "";

                        for (var i = 0; i < data.disks.length; i++) {
                            var disk = data.disks[i];
                            var diskDiv = document.createElement("div");

                            diskDiv.innerHTML =
                                '<label for="usage">' + disk.used_space_readable + '/' + disk.total_space_readable + '</label>' +
                                '<progress style="width:100%;box-sizing:border-box;" class="disk_usage" max="' + disk.total_space + '" value="' + disk.used_space + '"></progress>';

                            disksContainer.appendChild(diskDiv);
                        }
                    } catch (e) {
                        console.error("JSON parse error:", e);
                    }
                } else {
                    console.error("Failed to fetch system info. Status:", xhr.status);
                }
            }
        };

        xhr.onerror = function () {
            console.error("Request failed while fetching system info.");
        };

        xhr.send();
    }

    updateInfo();
    setInterval(updateInfo, 2500);
});