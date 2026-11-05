# Downloads the bundled Google Fonts (all SIL OFL) next to this script. Run once: powershell -ExecutionPolicy Bypass -File fonts/fetch.ps1
$ErrorActionPreference = 'Continue'; $ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$base = 'https://raw.githubusercontent.com/google/fonts/main/ofl'
$list = @'
anton/Anton-Regular.ttf
bebasneue/BebasNeue-Regular.ttf
playfairdisplay/PlayfairDisplay%5Bwght%5D.ttf
playfairdisplay/PlayfairDisplay-Italic%5Bwght%5D.ttf
cormorantgaramond/CormorantGaramond%5Bwght%5D.ttf
cormorantgaramond/CormorantGaramond-Italic%5Bwght%5D.ttf
montserrat/Montserrat%5Bwght%5D.ttf
oswald/Oswald%5Bwght%5D.ttf
fraunces/Fraunces%5BSOFT%2CWONK%2Copsz%2Cwght%5D.ttf
syne/Syne%5Bwght%5D.ttf
dmserifdisplay/DMSerifDisplay-Regular.ttf
abrilfatface/AbrilFatface-Regular.ttf
spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf
inter/Inter%5Bopsz%2Cwght%5D.ttf
caveat/Caveat%5Bwght%5D.ttf
pacifico/Pacifico-Regular.ttf
cinzel/Cinzel%5Bwght%5D.ttf
bungee/Bungee-Regular.ttf
italiana/Italiana-Regular.ttf
poppins/Poppins-SemiBold.ttf
poppins/Poppins-Light.ttf
poppins/Poppins-Bold.ttf
raleway/Raleway%5Bwght%5D.ttf
dmsans/DMSans%5Bopsz%2Cwght%5D.ttf
unbounded/Unbounded%5Bwght%5D.ttf
bodonimoda/BodoniModa%5Bopsz%2Cwght%5D.ttf
instrumentserif/InstrumentSerif-Regular.ttf
instrumentserif/InstrumentSerif-Italic.ttf
gloock/Gloock-Regular.ttf
youngserif/YoungSerif-Regular.ttf
lora/Lora%5Bwght%5D.ttf
ebgaramond/EBGaramond%5Bwght%5D.ttf
marcellus/Marcellus-Regular.ttf
josefinsans/JosefinSans%5Bwght%5D.ttf
bigshouldersdisplay/BigShouldersDisplay%5Bwght%5D.ttf
staatliches/Staatliches-Regular.ttf
monoton/Monoton-Regular.ttf
orbitron/Orbitron%5Bwght%5D.ttf
bricolagegrotesque/BricolageGrotesque%5Bopsz%2Cwdth%2Cwght%5D.ttf
sora/Sora%5Bwght%5D.ttf
manrope/Manrope%5Bwght%5D.ttf
outfit/Outfit%5Bwght%5D.ttf
plusjakartasans/PlusJakartaSans%5Bwght%5D.ttf
lexend/Lexend%5Bwght%5D.ttf
spacemono/SpaceMono-Regular.ttf
spacemono/SpaceMono-Bold.ttf
yesevaone/YesevaOne-Regular.ttf
rozhaone/RozhaOne-Regular.ttf
limelight/Limelight-Regular.ttf
juliussansone/JuliusSansOne-Regular.ttf
tenorsans/TenorSans-Regular.ttf
forum/Forum-Regular.ttf
greatvibes/GreatVibes-Regular.ttf
dancingscript/DancingScript%5Bwght%5D.ttf
sacramento/Sacramento-Regular.ttf
lobster/Lobster-Regular.ttf
bangers/Bangers-Regular.ttf
chonburi/Chonburi-Regular.ttf
kalnia/Kalnia%5Bwdth%2Cwght%5D.ttf
rubikmonoone/RubikMonoOne-Regular.ttf
righteous/Righteous-Regular.ttf
playfairdisplaysc/PlayfairDisplaySC-Regular.ttf
cormorantinfant/CormorantInfant%5Bwght%5D.ttf
italianno/Italianno-Regular.ttf
abeezee/ABeeZee-Regular.ttf
worksans/WorkSans%5Bwght%5D.ttf
archivoblack/ArchivoBlack-Regular.ttf
leaguegothic/LeagueGothic%5Bwdth%5D.ttf
ibmplexserif/IBMPlexSerif-Light.ttf
newsreader/Newsreader%5Bopsz%2Cwght%5D.ttf
gfsdidot/GFSDidot-Regular.ttf
zillaslab/ZillaSlab-Bold.ttf
vollkorn/Vollkorn%5Bwght%5D.ttf
figtree/Figtree%5Bwght%5D.ttf
redhatdisplay/RedHatDisplay%5Bwght%5D.ttf
epilogue/Epilogue%5Bwght%5D.ttf
'@
foreach ($p in ($list -split "`r?`n" | Where-Object { $_ })) {
    $n = ($p -split '/')[-1] -replace '%5B.*%5D', '-Var' -replace '%5B.*', ''
    $f = Join-Path $PSScriptRoot $n
    if (-not (Test-Path $f)) { try { Invoke-WebRequest -UseBasicParsing -Uri "$base/$p" -OutFile $f } catch { Write-Warning "$n failed: $_" } }
}
