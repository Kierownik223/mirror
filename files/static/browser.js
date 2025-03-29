function openDialog() {
    document.getElementById("browse_dialog").style.display = "block";
}

function closeDialog() {
    document.getElementById("browse_dialog").style.display = "none";
}

function selectPath() {
    const iframe = document.getElementById("browser_iframe").contentWindow;
    const pathInput = iframe.document.querySelector("input[name='path']");
    
    if (pathInput) {
        document.getElementById("path").value = pathInput.value;
    }

    closeDialog();
}
