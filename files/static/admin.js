document.addEventListener("DOMContentLoaded", async function () {
    try {
        const response = await fetch('/api/sysinfo');
        const data = await response.json();

        const disksContainer = document.getElementById("disks");
        disksContainer.innerHTML = "";
        data.disks.forEach(disk => {
            const diskDiv = document.createElement("div");
            diskDiv.innerHTML = `
                    <label for="usage">${disk.used_space_readable}/${disk.total_space_readable}</label>
                    <progress style="width:100%;box-sizing:border-box;" class="disk_usage" max="${disk.total_space}" value="${disk.used_space}"></progress>
                `;
            disksContainer.appendChild(diskDiv);
        });
    } catch (error) {
        console.error("Error fetching system information:", error);
    }
});