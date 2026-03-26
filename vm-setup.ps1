# sd2dinit Artix Linux test VM setup
# Run as Administrator in PowerShell

$VmName   = "artix-dinit-test"
$VhdPath  = "C:\Hyper-V\$VmName\$VmName.vhdx"
$IsoDir   = "C:\Hyper-V\$VmName"
$IsoPath  = "$IsoDir\artix-base-dinit.iso"
# Check https://artixlinux.org/download.php for the latest base-dinit ISO URL
$IsoUrl   = "https://download.artixlinux.org/iso/artix-base-dinit-20250101-x86_64.iso"

# Create directory
New-Item -ItemType Directory -Force -Path $IsoDir | Out-Null

# Download ISO (~700 MB)
Write-Host "Downloading Artix dinit ISO (~700 MB)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $IsoUrl -OutFile $IsoPath -UseBasicParsing

# Create VHD
Write-Host "Creating 20 GB virtual disk..." -ForegroundColor Cyan
New-VHD -Path $VhdPath -SizeBytes 20GB -Dynamic | Out-Null

# Create VM (Generation 1 for broad ISO compatibility)
Write-Host "Creating VM..." -ForegroundColor Cyan
New-VM -Name $VmName `
       -MemoryStartupBytes 2GB `
       -Generation 1 `
       -SwitchName "Default Switch" | Out-Null

Set-VM -Name $VmName -ProcessorCount 2 -CheckpointType Disabled
Add-VMHardDiskDrive -VMName $VmName -Path $VhdPath
Set-VMDvdDrive -VMName $VmName -Path $IsoPath
Set-VMBios -VMName $VmName -StartupOrder @("CD", "IDE", "LegacyNetworkAdapter", "Floppy")

Write-Host ""
Write-Host "VM '$VmName' ready." -ForegroundColor Green
Write-Host ""
Write-Host "To start:"
Write-Host "  Start-VM -Name '$VmName'"
Write-Host "  vmconnect.exe localhost '$VmName'"
Write-Host ""
Write-Host "After installing Artix:"
Write-Host "  pacman -S git base-devel"
Write-Host "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
Write-Host "  git clone git@gitlab.cherkaoui.ch:HadiCherkaoui/sd2dinit.git"
Write-Host "  cd sd2dinit && cargo build --release"
Write-Host "  sudo cp target/release/sd2dinit /usr/bin/"
Write-Host "  sudo cp hooks/sd2dinit.hook /usr/share/libalpm/hooks/"
Write-Host "  sd2dinit convert /usr/lib/systemd/system/sshd.service --dry-run"
