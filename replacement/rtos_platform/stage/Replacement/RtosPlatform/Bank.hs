-- | Joint admission of the fixed bank and the loader entry veneers that enter it.
module Replacement.RtosPlatform.Bank
  ( Bank (..)
  , BankError (..)
  , Veneer (..)
  , AdmittedVeneer (..)
  , parseBank
  , bankEntries
  , loaderDomain
  , admitVeneers
  ) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified Replacement.FixedImage.Layout as Fixed
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLoaderBase, domainLoaderSize, domains)
import qualified Replacement.RtosPlatform.Aggregate as A
import Replacement.RtosPlatform.Union

-- | The admitted fixed body @[bankStart, bankEnd)@ and the symbols of the final link that
-- produced it.
data Bank = Bank
  { bankStart :: Word32
  , bankEnd :: Word32
  , bankSymbols :: [F.Symbol]
  }
  deriving (Eq, Show)

-- | A proposed entry word in a loader.
data Veneer = Veneer
  { veneerDomain :: Domain
  , veneerAddress :: GuestAddress
  , veneerWord :: Word32
  }
  deriving (Eq, Show)

data AdmittedVeneer = AdmittedVeneer
  { admittedEntry :: Entry
  , admittedVeneer :: Veneer
  , admittedTarget :: Word32
  }
  deriving (Eq, Show)

data BankError
  = BankSymbolsUnparsed String
  | BankBoundsInvalid Word32 Word32
  | VeneerUndeclared Veneer
  | VeneerRepeated Domain GuestAddress
  | VeneerNotBranch Domain Entry Word32
  | VeneerOutsideBank Domain Entry Word32
  | VeneerWrongDomain Domain Entry Word32
  | VeneerTargetMismatch Domain Entry Word32
  | BankBodyMissing Domain Entry
  | BankBodyWithoutVeneer Domain Entry
  deriving (Eq, Show)

-- | The body bounds of one final fixed-bank link.
parseBank :: B.ByteString -> Either BankError Bank
parseBank bytes = do
  symbols <- either (Left . BankSymbolsUnparsed) Right (F.parseSymbols bytes)
  let boundary name = either (Left . BankSymbolsUnparsed) (Right . F.symbolAddress) (F.lookupSymbol symbols name)
  start <- boundary "__fixed_body_start"
  end <- boundary "__fixed_body_end"
  unless (start == Fixed.imageBase + Fixed.headerBytes && start < end && end <= Fixed.imageLimit)
    (Left (BankBoundsInvalid start end))
  pure (Bank start end symbols)

bankEntries :: Union -> Domain -> [Entry]
bankEntries union domain = [entry | entry <- mapEntries (domainMap union domain), entryBody entry == A.BankBody]

-- | The domain whose decoded loader holds @address@.
loaderDomain :: GuestAddress -> Maybe Domain
loaderDomain (GuestAddress address) = case
  [ domain
  | domain <- domains
  , let GuestAddress base = domainLoaderBase domain
  , base <= address
  , toInteger address < toInteger base + toInteger (domainLoaderSize domain)
  ] of
  [domain] -> Just domain
  _ -> Nothing

-- | Admits each veneer as the entry word of a declared bank-bodied entry of its own domain
-- that branches to that domain's body inside the bank, and requires a veneer for every
-- declared entry whose body the bank links.
admitVeneers :: Union -> Bank -> [Veneer] -> Either BankError [AdmittedVeneer]
admitVeneers union bank veneers = do
  forM_ (zip ordered (drop 1 ordered)) $ \(left, right) ->
    when (key left == key right) (Left (VeneerRepeated (veneerDomain left) (veneerAddress left)))
  admitted <- forM veneers $ \veneer -> do
    let domain = veneerDomain veneer
    entry <- case [entry | entry <- bankEntries union domain, entryAddress entry == veneerAddress veneer] of
      [entry] -> Right entry
      _ -> Left (VeneerUndeclared veneer)
    let GuestAddress at = veneerAddress veneer
    target <- maybe (Left (VeneerNotBranch domain entry (veneerWord veneer))) Right (branchTarget at (veneerWord veneer))
    unless (bankStart bank <= target && target < bankEnd bank) (Left (VeneerOutsideBank domain entry target))
    case body entry of
      Nothing -> Left (BankBodyMissing domain entry)
      Just address | address == target -> pure (AdmittedVeneer entry veneer target)
      _ | Just target == other domain entry -> Left (VeneerWrongDomain domain entry target)
        | otherwise -> Left (VeneerTargetMismatch domain entry target)
  forM_ domains $ \domain ->
    forM_ (bankEntries union domain) $ \entry -> do
      let entered = any (\veneer -> veneerDomain veneer == domain && veneerAddress veneer == entryAddress entry) veneers
      when (body entry /= Nothing && not entered) (Left (BankBodyWithoutVeneer domain entry))
  pure admitted
  where
    key veneer = (veneerDomain veneer, veneerAddress veneer)
    ordered = sortOn key veneers
    body entry = case
      [ F.symbolAddress symbol
      | symbol <- bankSymbols bank
      , F.symbolName symbol == entrySymbol entry
      , F.symbolSize symbol > 0
      , bankStart bank <= F.symbolAddress symbol
      , toInteger (F.symbolAddress symbol) + toInteger (F.symbolSize symbol) <= toInteger (bankEnd bank)
      ] of
      [address] -> Just address
      _ -> Nothing
    other domain entry = case
      [ body counterpart
      | counterpartDomain <- domains
      , counterpartDomain /= domain
      , counterpart <- bankEntries union counterpartDomain
      , entryName counterpart == entryName entry
      ] of
      [address] -> address
      _ -> Nothing
