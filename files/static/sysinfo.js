document.addEventListener("DOMContentLoaded", function() {
    async function updateInfo() {
        try {
            const response = await fetch('/api/sysinfo');
            const data = await response.json();

            document.getElementById("used_mem_readable").textContent = data.used_mem_readable;
            document.getElementById("total_mem_readable").textContent = data.total_mem_readable;
            const memUsage = document.getElementById("mem_usage");
            memUsage.max = data.total_mem;
            memUsage.value = data.used_mem;

            const disksContainer = document.getElementById("disks");
            disksContainer.innerHTML = "";
            data.disks.forEach(disk => {
                const diskDiv = document.createElement("div");
                diskDiv.innerHTML = `
                    <label for="usage">Disk usage: ${disk.used_space_readable}/${disk.total_space_readable}</label>
                    <progress style="width:100%;box-sizing:border-box;" class="disk_usage" max="${disk.total_space}" value="${disk.used_space}"></progress>
                `;
                disksContainer.appendChild(diskDiv);
            });
        } catch (error) {
            console.error("Error fetching system information:", error);
        }
    }
    setInterval(updateInfo, 2500);
});