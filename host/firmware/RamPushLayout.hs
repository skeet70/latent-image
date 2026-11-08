-- | The RAM-push layout. The installer section holds for every payload kind:
-- menu/ram_push_installer.c linked at a 32-byte boundary, then its
-- publication table, rendezvous and mailbox. The runtime section is the
-- recipe bank, relocated D3 and Classic Chrome payload.
module RamPushLayout
  ( installerInstallOffset
  , installerRollbackOffset
  , installerStatusOffset
  , installerExtent
  , installerHasCallClosure
  , encodePublicationRecord
  , runtimeExecutableBase
  , runtimeDataBase
  , runtimeNameBase
  , runtimeBankEnd
  , runtimeInstallerBase
  , runtimeInstallerExtent
  , runtimeAllocationSize
  , safePublicationOrderFor
  , BankVariant
  , PublicationGroup (..)
  , bankVariant
  , publicationGroups
  , hasPublicationGroup
  , standardBankVariant
  , modalScopedBankVariant
  , lifecycleBankVariant
  , modalLifecycleBankVariant
  , modalEngineBankVariant
  , modalRenderBankVariant
  , bankVariantTag
  , parseBankVariantTag
  , bankVariantFromIdentity
  , renderHookAddresses
  , renderHookWordFor
  , menuEngineHookAddress
  , menuEngineHookWordFor
  , menuEngineRegionStart
  , menuEngineRegionExtent
  , menuEngineRegionStartFor
  , menuEngineRegionExtentFor
  , publicationCountFor
  , publicationActivationCountFor
  , runtimeInstallerExtentFor
  , runtimeAllocationSizeFor
  , safePublicationOrderForVariant
  , modalHookAddress
  , lifecycleHookAddresses
  , modalHookWordFor
  , captureHookAddress
  , PublicationAddress (..)
  , PublicationIndex (..)
  , publicationSetupCount
  , publicationActivationCount
  , publicationCount
  , staleFixedReferences
  ) where

import Control.Monad (when)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (elemIndex, isInfixOf, isSuffixOf, sort)
import Data.Word (Word32)

-- * Installer

-- | Entry offsets from the installer start.
installerInstallOffset, installerRollbackOffset, installerStatusOffset :: Word32
installerInstallOffset = 0x600
installerRollbackOffset = 0x838
installerStatusOffset = 0x864

-- | The installer's code and read-only data from its start.
installerCodeExtent :: Word32
installerCodeExtent = 0xa84

-- | The installer's extent from its start for a payload of `count`
-- publications: code, the 16-byte publication records, the 128-byte
-- rendezvous and the 104-byte mailbox with its trailing byte, at the
-- alignments of the payload linker scripts.
installerExtent :: Int -> Word32
installerExtent count =
  align32 (align32 (installerCodeExtent + 16 * fromIntegral count) + 128 + 104 + 1)
 where
  align32 offset = (offset + 31) .&. 0xffffffe0

installerHasCallClosure :: String -> Bool
installerHasCallClosure disassembly =
  and
    [ requires "ram_push_install" ["execute_operation", "publish_mailbox"]
    , requires "ram_push_rollback" ["execute_operation", "publish_mailbox"]
    , requires "execute_operation" ["run_rendezvous"]
    , requires "run_rendezvous"
        ["post_peer_park", "wait_for_signal", "ram_push_rendezvous_locked", "publish_words_locked"]
    , ordered "run_rendezvous" ["ram_push_rendezvous_locked", "publish_words_locked"]
    , requires "ram_push_rendezvous_locked" ["validate_old_words", "validate_rollback_words"]
    , requires "publish_words_locked" ["store_new_words", "restore_old_words", "synchronize_current_core"]
    , requires "peer_park_action" ["synchronize_current_core", "ram_push_cpu1_window_exit"]
    , functionPresent "peer_park_complete"
    , contains "post_peer_park" "blx"
    , masked "run_rendezvous"
    , masked "peer_park_action"
    , symbolPresent "ram_push_cpu0_window_exit"
    , symbolPresent "ram_push_cpu1_window_exit"
    ]
 where
  requires name dependencies =
    maybe False
      (\body -> all (\dependency -> ("<" ++ dependency ++ ">") `isInfixOf` body) dependencies)
      (functionBody name)
  ordered name dependencies =
    maybe False (orderedReferences dependencies . lines) (functionBody name)
  orderedReferences [] _ = True
  orderedReferences (dependency : rest) body =
    case dropWhile (not . isInfixOf ("<" ++ dependency ++ ">")) body of
      [] -> False
      _ : after -> orderedReferences rest after
  contains name instruction =
    maybe False (isInfixOf instruction) (functionBody name)
  masked name =
    maybe False
      (\body -> "cpsid" `isInfixOf` body && "cpsie" `isInfixOf` body)
      (functionBody name)
  functionPresent name = maybe False (const True) (functionBody name)
  symbolPresent name = ("<" ++ name ++ ">:") `isInfixOf` disassembly
  functionBody name =
    case dropWhile (not . isSuffixOf ("<" ++ name ++ ">:")) (lines disassembly) of
      [] -> Nothing
      _ : body -> Just (unlines (takeWhile (not . isSuffixOf ">:") body))

encodePublicationRecord :: Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
encodePublicationRecord address oldWord newWord phase =
  word32 address <> word32 oldWord <> word32 newWord <> word32 phase

-- * Recipe bank and D3 runtime

runtimeExecutableBase :: Word32 -> Word32
runtimeExecutableBase base = base + 0x500

runtimeDataBase :: Word32 -> Word32
runtimeDataBase base = base + 0xa800

runtimeNameBase :: Word32 -> Word32
runtimeNameBase base =
  let minimumName = base + 0xbb00
      candidate = (minimumName .&. 0xffff0000) .|. 0xb784
   in if candidate < minimumName then candidate + 0x10000 else candidate

-- | Exclusive end of writable generated bank data.
runtimeBankEnd :: Word32 -> Word32
runtimeBankEnd base = runtimeNameBase base + 0x400

runtimeInstallerBase :: Word32 -> Word32
runtimeInstallerBase base = (runtimeNameBase base + 0x800 + 31) .&. 0xffffffe0

runtimeInstallerExtent :: Word32
runtimeInstallerExtent = installerExtent publicationCount

runtimeAllocationSize :: Word32
runtimeAllocationSize = 0x1dc98

data PublicationGroup = LifecyclePublications | Modal75Publication | MenuEnginePublication | RenderPublication
  deriving (Eq, Ord, Show)

newtype BankVariant = BankVariant [PublicationGroup]
  deriving (Eq, Show)

-- | Render requires the modal75 and menu-engine groups.
bankVariant :: [PublicationGroup] -> Either String BankVariant
bankVariant groups = do
  let ordered = sort groups
  when (or (zipWith (==) ordered (drop 1 ordered))) $
    Left "duplicate publication group"
  when (RenderPublication `elem` ordered && ordered /= [Modal75Publication, MenuEnginePublication, RenderPublication]) $
    Left "the render group requires exactly modal75 and menu-engine"
  when (MenuEnginePublication `elem` ordered && filter (/= RenderPublication) ordered /= [Modal75Publication, MenuEnginePublication]) $
    Left "the menu-engine group requires exactly the modal75 group"
  pure (BankVariant ordered)

publicationGroups :: BankVariant -> [PublicationGroup]
publicationGroups (BankVariant groups) = groups

hasPublicationGroup :: PublicationGroup -> BankVariant -> Bool
hasPublicationGroup group = elem group . publicationGroups

standardBankVariant, modalScopedBankVariant, lifecycleBankVariant, modalLifecycleBankVariant, modalEngineBankVariant :: BankVariant
standardBankVariant = BankVariant []
modalScopedBankVariant = BankVariant [Modal75Publication]
lifecycleBankVariant = BankVariant [LifecyclePublications]
modalLifecycleBankVariant = BankVariant [LifecyclePublications, Modal75Publication]
modalEngineBankVariant = BankVariant [Modal75Publication, MenuEnginePublication]

modalRenderBankVariant :: BankVariant
modalRenderBankVariant = BankVariant [Modal75Publication, MenuEnginePublication, RenderPublication]

bankVariantTag :: BankVariant -> String
bankVariantTag variant
  | variant == standardBankVariant = "standard"
  | otherwise = concatTags (map groupTag (publicationGroups variant))
 where
  groupTag LifecyclePublications = "lifecycle"
  groupTag Modal75Publication = "modal75"
  groupTag MenuEnginePublication = "menu-engine"
  groupTag RenderPublication = "render"
  concatTags [] = ""
  concatTags [tag] = tag
  concatTags (tag : rest) = tag ++ "," ++ concatTags rest

parseBankVariantTag :: String -> Either String BankVariant
parseBankVariantTag tag = case filter ((== tag) . bankVariantTag) variants of
  [variant] -> Right variant
  _ -> Left "unknown bank variant tag"
 where
  variants = [standardBankVariant, modalScopedBankVariant, lifecycleBankVariant, modalLifecycleBankVariant, modalEngineBankVariant, modalRenderBankVariant]

-- | Untagged schema-1 packages retain their legacy count identities.
bankVariantFromIdentity :: Maybe String -> Int -> Either String BankVariant
bankVariantFromIdentity tag count = do
  variant <- case tag of
    Just identity -> parseBankVariantTag identity
    Nothing -> case count of
      223 -> Right standardBankVariant
      224 -> Right modalScopedBankVariant
      225 -> Right modalEngineBankVariant
      226 -> Right lifecycleBankVariant
      227 -> Right modalLifecycleBankVariant
      _ -> Left "unsupported legacy bank publication count"
  when (publicationCountFor variant /= count) $
    Left "bank variant tag and publication count disagree"
  pure variant

publicationCountFor :: BankVariant -> Int
publicationCountFor variant = publicationCount + sum (map groupCount (publicationGroups variant))
 where
  groupCount LifecyclePublications = 3
  groupCount Modal75Publication = 1
  groupCount MenuEnginePublication = 1
  groupCount RenderPublication = 3

publicationActivationCountFor :: BankVariant -> Int
publicationActivationCountFor variant = publicationCountFor variant - publicationSetupCount

runtimeInstallerExtentFor :: BankVariant -> Word32
runtimeInstallerExtentFor variant
  | variant == standardBankVariant = runtimeInstallerExtent
  | otherwise = installerExtent (publicationCountFor variant)

runtimeAllocationSizeFor :: BankVariant -> Word32
runtimeAllocationSizeFor variant
  | hasPublicationGroup MenuEnginePublication variant =
      menuEngineRegionStart 0 + menuEngineRegionExtent
  | otherwise =
      runtimeAllocationSize + runtimeInstallerExtentFor variant - runtimeInstallerExtent
        + if variant == modalLifecycleBankVariant then 32 else 0

newtype PublicationAddress = PublicationAddress { publicationAddress :: Word32 }
  deriving (Eq, Ord, Show)

newtype PublicationIndex = PublicationIndex { publicationIndex :: Int }
  deriving (Eq, Ord, Show)

-- | The capture entry hook appended after every existing logical publication.
captureHookAddress :: PublicationAddress
captureHookAddress = PublicationAddress 0x00e9e038

-- | The reviewed bank, Classic Chrome and D3 order before the capture hook.
legacySafePublicationOrder :: [PublicationIndex]
legacySafePublicationOrder =
  map PublicationIndex $
    [0 .. 35] <> [55 .. 68] <> [88] <> [36 .. 53]
      <> [97 .. 221]
      <> [75,76,72,73,74,69,70,71,77,78,79,80,81,82,83,89,90,91,92,93,94,95,96,84,85,86,87,54]

legacyPublicationSetupCount :: Int
legacyPublicationSetupCount = 131

legacyPublicationCount :: Int
legacyPublicationCount = 222

publicationSetupCount :: Int
publicationSetupCount = legacyPublicationSetupCount

publicationActivationCount :: Int
publicationActivationCount = legacyPublicationCount + 1 - legacyPublicationSetupCount

publicationCount :: Int
publicationCount = legacyPublicationCount + 1

-- | Map the reviewed order onto the actual logical addresses and insert the
-- capture hook after the ready setup records, before prior activations.
safePublicationOrderFor :: [PublicationAddress] -> Either String [PublicationIndex]
safePublicationOrderFor addresses = do
  when (length addresses /= publicationCount) $
    Left ("publication count " ++ show (length addresses) ++ ", not " ++ show publicationCount)
  let sortedAddresses = sort addresses
  when (or (zipWith (==) sortedAddresses (drop 1 sortedAddresses))) $
    Left "duplicate logical publication address"
  captureIndex <- maybe
    (Left "capture hook publication missing")
    (Right . PublicationIndex)
    (elemIndex captureHookAddress addresses)
  let adjust (PublicationIndex oldIndex) =
        PublicationIndex (if oldIndex < publicationIndex captureIndex then oldIndex else oldIndex + 1)
      mappedSetup = map adjust (take legacyPublicationSetupCount legacySafePublicationOrder)
      mappedActivation = map adjust (drop legacyPublicationSetupCount legacySafePublicationOrder)
      order = mappedSetup ++ [captureIndex] ++ mappedActivation
  when (sort (map publicationIndex order) /= [0 .. publicationCount - 1]) $
    Left "derived publication order is not a permutation"
  pure order

modalHookAddress :: PublicationAddress
modalHookAddress = PublicationAddress 0x00bfb028

-- | The engine wrapper `FUN_00bf95c8`.
menuEngineHookAddress :: PublicationAddress
menuEngineHookAddress = PublicationAddress 0x00bf95c8

-- | Tree, picker, and pane entries are the final render publications.
renderHookAddresses :: [PublicationAddress]
renderHookAddresses = map PublicationAddress [0x01164e1c, 0x01163f2c, 0x011662a0]

-- | The owned engine's code and tree, placed after the scoped modal
-- allocation's end.
menuEngineRegionStart :: Word32 -> Word32
menuEngineRegionStart base = base + ((runtimeAllocationSizeFor modalScopedBankVariant + 31) .&. 0xffffffe0)

menuEngineRegionExtent :: Word32
menuEngineRegionExtent = 0x10000

menuEngineRegionStartFor :: BankVariant -> Word32 -> Word32
menuEngineRegionStartFor variant base
  | hasPublicationGroup RenderPublication variant =
      base + ((runtimeAllocationSize + runtimeInstallerExtentFor variant - runtimeInstallerExtent + 31) .&. 0xffffffe0)
  | otherwise = menuEngineRegionStart base

menuEngineRegionExtentFor :: BankVariant -> Word32
menuEngineRegionExtentFor variant =
  menuEngineRegionStart 0 + menuEngineRegionExtent - menuEngineRegionStartFor variant 0

lifecycleHookAddresses :: [PublicationAddress]
lifecycleHookAddresses = map PublicationAddress [0x00e99e8c, 0x00e6a754, 0x00e6bb90]

safePublicationOrderForVariant :: BankVariant -> [PublicationAddress] -> Either String [PublicationIndex]
safePublicationOrderForVariant variant addresses = do
  when (length addresses /= publicationCountFor variant) $
    Left "selected publication count differs"
  let sortedAddresses = sort addresses
  when (or (zipWith (==) sortedAddresses (drop 1 sortedAddresses))) $
    Left "duplicate logical publication address"
  let modalSelected = hasPublicationGroup Modal75Publication variant
      engineSelected = hasPublicationGroup MenuEnginePublication variant
      finalHooks = [modalHookAddress | modalSelected] <> [menuEngineHookAddress | engineSelected]
        <> [hook | hasPublicationGroup RenderPublication variant, hook <- renderHookAddresses]
  when (not (null finalHooks) && drop (length addresses - length finalHooks) addresses /= finalHooks) $
    Left "scoped modal hook must occur once at the final logical index"
  when (not modalSelected && modalHookAddress `elem` addresses) $
    Left "modal hook present without its publication group"
  when (not engineSelected && menuEngineHookAddress `elem` addresses) $
    Left "menu engine hook present without its publication group"
  let lifecycleAddresses = if hasPublicationGroup LifecyclePublications variant then lifecycleHookAddresses else []
      selectedAddresses = lifecycleAddresses <> finalHooks
  when (drop publicationCount addresses /= selectedAddresses) $
    Left "publication group destinations or logical order differ"
  order <- safePublicationOrderFor (take publicationCount addresses)
  pure (order <> map PublicationIndex [publicationCount .. publicationCountFor variant - 1])

modalHookWordFor :: BankVariant -> Word32 -> Word32 -> Word32 -> Either String Word32
modalHookWordFor variant base target source = do
  when (not (hasPublicationGroup Modal75Publication variant)) $
    Left "modal hook requires the modal75 publication group"
  when (source /= 0xe92d4ff0) $
    Left "stock modal entry is not push r4-r11,lr"
  when (target .&. 3 /= 0 || target < runtimeExecutableBase base || target >= runtimeDataBase base) $
    Left "modal target leaves bank executable storage"
  hookBranchWord variant base modalHookAddress target

-- | The branch at `0xbf95c8` to the owned engine entry in the engine region.
menuEngineHookWordFor :: BankVariant -> Word32 -> Word32 -> Word32 -> Either String Word32
menuEngineHookWordFor variant base target source = do
  when (not (hasPublicationGroup MenuEnginePublication variant)) $
    Left "menu engine hook requires the menu-engine publication group"
  when (source /= 0xe92d4818) $
    Left "stock engine wrapper entry is not push r3,r4,r11,lr"
  when (target .&. 3 /= 0 || target < menuEngineRegionStartFor variant base || target >= menuEngineRegionStartFor variant base + menuEngineRegionExtentFor variant) $
    Left "menu engine target leaves the engine region"
  hookBranchWord variant base menuEngineHookAddress target

renderHookWordFor :: BankVariant -> Word32 -> PublicationAddress -> Word32 -> Word32 -> Either String Word32
renderHookWordFor variant base hook target source = do
  when (not (hasPublicationGroup RenderPublication variant) || hook `notElem` renderHookAddresses) $
    Left "render hook requires the render publication group and a render entry"
  when (source /= 0xe92d4ff0) $
    Left "stock render entry is not push r4-r11,lr"
  when (target .&. 3 /= 0 || target < menuEngineRegionStartFor variant base || target >= menuEngineRegionStartFor variant base + menuEngineRegionExtentFor variant) $
    Left "render target leaves the engine region"
  hookBranchWord variant base hook target

hookBranchWord :: BankVariant -> Word32 -> PublicationAddress -> Word32 -> Either String Word32
hookBranchWord variant base (PublicationAddress hook) target = do
  when (base .&. 7 /= 0 || base > maxBound - runtimeAllocationSizeFor variant) $
    Left "scoped allocation leaves the aligned address domain"
  let allocationEnd = toInteger base + toInteger (runtimeAllocationSizeFor variant)
      delta = toInteger target - toInteger hook - 8
  when (not (toInteger hook + 4 <= toInteger base || toInteger hook >= allocationEnd)) $
    Left "hook overlaps the allocation"
  when (delta `mod` 4 /= 0 || delta < -0x2000000 || delta > 0x1fffffc) $
    Left "hook leaves the A32 branch domain"
  pure (0xea000000 .|. (fromInteger (delta `div` 4) .&. 0x00ffffff))

staleFixedReferences :: Word32 -> Word32 -> B.ByteString -> [(Int, Word32)]
staleFixedReferences low high bytes = direct <> movPairs
 where
  direct =
    [ (offset, value)
    | offset <- [0 .. B.length bytes - 4]
    , let value = wordAt offset
    , low <= value
    , value < high
    ]
  movPairs =
    [ (offset, value)
    | offset <- [0, 4 .. B.length bytes - 8]
    , let lower = wordAt offset
    , isMovw lower
    , upperOffset <- [offset + 4, offset + 8 .. min (offset + 32) (B.length bytes - 4)]
    , let upper = wordAt upperOffset
    , isMovt upper
    , register lower == register upper
    , let value = (immediate upper `shiftL` 16) .|. immediate lower
    , low <= value
    , value < high
    ]
  wordAt offset =
    sum
      [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]
  isMovw word = word .&. 0x0ff00000 == 0x03000000
  isMovt word = word .&. 0x0ff00000 == 0x03400000
  register word = (word `shiftR` 12) .&. 15
  immediate word = ((word `shiftR` 16) .&. 15) `shiftL` 12 .|. (word .&. 0xfff)

word32 :: Word32 -> B.ByteString
word32 word = B.pack [fromIntegral ((word `div` (256 ^ index)) `mod` 256) | index <- [0 .. 3 :: Int]]
