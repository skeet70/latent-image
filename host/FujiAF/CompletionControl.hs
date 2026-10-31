-- | Declared loader-A completion controls built from a reviewed X-M1 1.50
-- container. Each changes only its loader-A word, the delayed control also
-- the record-1 veneer extent, and the main checksum.
module FujiAF.CompletionControl
  ( CompletionControl (..)
  , completionControlName
  , parseCompletionControl
  , completionValue
  , completionStoreWord
  , heldGate
  , veneerEntry
  , veneerRelease
  , veneerEnd
  , delayIterations
  , ControlArtifact
  , controlBytes
  , controlParsed
  , controlLoaderPatches
  , controlVeneerExtent
  , buildCompletionControl
  , validateDelayVeneer
  , delayVeneer
  ) where

import Control.Monad (forM_, unless)
import Data.Bits (complement, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified FujiAF.CompletionDelayImage as Image
import FujiAF.FirstStage
import qualified FujiAF.UpdateContainer as U

data CompletionControl = HeldCompletion | DelayedCompletion | NonOneCompletion
  deriving (Eq, Show, Enum, Bounded)

completionControlName :: CompletionControl -> String
completionControlName HeldCompletion = "held"
completionControlName DelayedCompletion = "delayed"
completionControlName NonOneCompletion = "non-one"

parseCompletionControl :: String -> Either String CompletionControl
parseCompletionControl name = case [control | control <- [minBound .. maxBound], completionControlName control == name] of
  [control] -> Right control
  _ -> Left ("unsupported completion control: " ++ name)

loaderA :: LoaderId
loaderA = LoaderId 0

originalMove, originalStore, nonOneStore, selfBranch :: Word32
originalMove = 0xe3a05001
originalStore = 0xe58350c4
nonOneStore = 0xe58330c4
selfBranch = 0xeafffffe

-- | Loader A's `mov r5,#1` at 0x40a00, the held self-branch.
heldGate :: Word32
heldGate = 0x40a00

completionStore :: Word32
completionStore = 0x40a08

-- | The value loader A's 0x40a08 store writes to 0x400a00c4; the non-one
-- store writes r3, the 0x40afc literal 0x400a0000.
completionValue :: Maybe CompletionControl -> Word32
completionValue (Just NonOneCompletion) = 0x400a0000
completionValue _ = 1

completionStoreWord :: Maybe CompletionControl -> Word32
completionStoreWord (Just NonOneCompletion) = nonOneStore
completionStoreWord _ = originalStore

veneerEntry, veneerRelease, veneerEnd :: Word32
veneerEntry = 0x180
veneerRelease = 0x194
veneerEnd = 0x19c

delayIterations :: Word32
delayIterations = 0x10000

veneerWords :: [Word32]
veneerWords =
  [ 0xe10f3000 -- mrs r3, cpsr
  , 0xe3a05801 -- mov r5, #0x10000
  , 0xe2555001 -- subs r5, r5, #1
  , 0x1afffffd -- bne 0x188
  , 0xe128f003 -- msr cpsr_f, r3
  , originalMove
  , 0xe12fff1e -- bx lr
  ]

branchLink :: Word32 -> Word32 -> Word32
branchLink from to = 0xeb000000 .|. (((to - (from + 8)) `shiftR` 2) .&. 0xffffff)

-- | The linked veneer, checked against its reviewed words and symbols.
validateDelayVeneer :: B.ByteString -> B.ByteString -> Either String B.ByteString
validateDelayVeneer bytes symbols = do
  require (bytes == B.concat (map U.putWord32 veneerWords)) "linked completion delay veneer differs from its reviewed words"
  rows <- parseSymbols symbols
  forM_ [("xm1_completion_delay_entry", veneerEntry), ("xm1_completion_delay_release", veneerRelease), ("xm1_completion_delay_end", veneerEnd)] $
    \(name, address) -> do
      symbol <- lookupSymbol rows name
      require (symbolAddress symbol == address) (name ++ " differs from its declared address")
  entry <- lookupSymbol rows "xm1_completion_delay_entry"
  require (symbolSize entry == veneerEnd - veneerEntry && fromIntegral (B.length bytes) == veneerEnd - veneerEntry)
    "completion delay veneer extent differs"
  pure bytes

data ControlArtifact = ControlArtifact
  { controlBytes :: B.ByteString
  , controlParsed :: U.Parsed
  , controlLoaderPatches :: [LoaderWordPatch]
  -- | Record-1 slot offset and length of the veneer, if any.
  , controlVeneerExtent :: Maybe (Int, Int)
  }

loaderPatch :: CompletionControl -> Either String LoaderWordPatch
loaderPatch HeldCompletion = loaderWordPatch loaderA heldGate originalMove selfBranch
loaderPatch DelayedCompletion = loaderWordPatch loaderA heldGate originalMove (branchLink heldGate veneerEntry)
loaderPatch NonOneCompletion = loaderWordPatch loaderA completionStore originalStore nonOneStore

-- | The linked veneer of "FujiAF.CompletionDelayImage", validated.
delayVeneer :: Either String B.ByteString
delayVeneer = validateDelayVeneer Image.veneerBytes Image.veneerSymbols

veneerPatch :: CompletionControl -> Either String (Maybe ExpectedMediaPatch)
veneerPatch DelayedCompletion = do
  veneer <- delayVeneer
  offset <- slotOffset (fromIntegral veneerEntry)
  Just <$> expectedMediaPatch offset (U.MediaImage (B.replicate (B.length veneer) 0xff)) (U.MediaImage veneer)
veneerPatch _ = Right Nothing

checksPass :: [U.Check] -> Either String ()
checksPass results = forM_ results $ \check -> require (U.checkPassed check) ("container contract: " ++ U.checkField check)

buildCompletionControl :: CompletionControl -> U.Parsed -> Either String ControlArtifact
buildCompletionControl control parsed = do
  checksPass (U.checks parsed ++ U.carriedChecks parsed)
  patch <- loaderPatch control
  veneer <- veneerPatch control
  withVeneer <- maybe (Right (U.parsedParts parsed)) (\expected -> replaceExpectedFirstRecord [expected] (U.parsedParts parsed)) veneer
  updated <- applyLoaderWordPatches [patch] withVeneer
  rebuilt <- U.rebuildContainer parsed updated
  reparsed <- U.parseContainer rebuilt
  checksPass (U.checks reparsed ++ U.carriedChecks reparsed)
  let extent = (fromIntegral veneerEntry, fromIntegral (veneerEnd - veneerEntry)) <$ veneer
      preTable = [(firstRecordSlotOffset + offset, size) | Just (offset, size) <- [extent]]
  verifyPreTablePatches preTable [patch] parsed reparsed
  forM_ extent $ \(offset, size) -> do
    slot <- firstRecordSlotBytes (U.parsedParts reparsed)
    require (B.map complement (B.take size (B.drop offset slot)) == Image.veneerBytes)
      "staged completion delay veneer differs from the linked bytes"
  require (B.length rebuilt == U.parsedFileLength parsed) "completion control DAT length differs"
  pure (ControlArtifact rebuilt reparsed [patch] extent)

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)
